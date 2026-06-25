import { randomUUID } from "node:crypto";
import type { ChildProcess } from "node:child_process";
import type {
  DispatchResult,
  FinalizeCtx,
  JobSpec,
  JobStatus,
  PollResult,
  RunOutput,
} from "./types.js";
import type { Backend, BackendResult, ProgressSnapshotRaw } from "./backends/types.js";
import type { ProgressSink, ProgressUpdate } from "./progress.js";
import { finalizeRun as defaultFinalize, baseOutput } from "./finalize.js";
import { clampWait } from "./util.js";

const COMPLETED_CAP = 100;
const DEFAULT_WAIT_TIMEOUT = 120000;

// ---- injectable clock (for deterministic tests) ----
export type TimerHandle = unknown;
export interface Clock {
  now(): number;
  setTimer(ms: number, cb: () => void): TimerHandle;
  clearTimer(h: TimerHandle): void;
}

const realClock: Clock = {
  now: () => Date.now(),
  setTimer: (ms, cb) => setTimeout(cb, ms),
  clearTimer: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
};

interface Job {
  id: string;
  status: JobStatus;
  spec: JobSpec;
  child: ChildProcess | null;
  startedAt: number;
  lastTool: string | null;
  tokensSoFar: number;
  lastAssistant: string | null;
  filesTouched: string[];
  phase: string | null;
  lastEventAt: number;
  terminationReason: "CANCELLED" | "STALLED" | null;
  idleTimer: TimerHandle | null;
  completion: Promise<RunOutput>;
  terminalOutput: RunOutput | null;
  sinks: Set<ProgressSink>;
}

export interface DispatchOpts {
  sink?: ProgressSink;
  signal?: AbortSignal;
}

export interface WaitOpts {
  sink?: ProgressSink;
  signal?: AbortSignal;
}

export interface JobRegistry {
  dispatch(spec: JobSpec, opts?: DispatchOpts): Promise<DispatchResult>;
  poll(jobId: string): PollResult;
  cancel(jobId: string): Promise<PollResult>;
  wait(jobId: string, timeoutMs?: number, opts?: WaitOpts): Promise<PollResult>;
  waitAny(
    jobIds: string[],
    timeoutMs?: number,
    opts?: WaitOpts,
  ): Promise<{ jobs: Record<string, PollResult>; firstDone?: string }>;
  waitAll(
    jobIds: string[],
    timeoutMs?: number,
    opts?: WaitOpts,
  ): Promise<{ jobs: Record<string, PollResult>; allDone: boolean }>;
  killAll(): void;
}

export interface RegistryDeps {
  backend: Backend;
  deadlineMs: number;
  idleMs: number | null;
  clock?: Clock;
  finalize?: (res: BackendResult, ctx: FinalizeCtx) => Promise<RunOutput>;
}

export function makeJobRegistry(deps: RegistryDeps): JobRegistry {
  const { backend, deadlineMs, idleMs } = deps;
  const clock = deps.clock ?? realClock;
  const finalize = deps.finalize ?? defaultFinalize;

  const active = new Map<string, Job>();
  const completed = new Map<string, Job>();
  const pathLock = new Map<string, string>();

  function getJob(jobId: string): Job | undefined {
    return active.get(jobId) ?? completed.get(jobId);
  }

  function retire(job: Job, out: RunOutput): void {
    if (job.idleTimer) {
      clock.clearTimer(job.idleTimer);
      job.idleTimer = null;
    }
    job.status = job.terminationReason ?? out.status;
    job.terminalOutput = out;
    job.child = null;
    active.delete(job.id);
    if (
      job.spec.isWrite &&
      job.spec.path &&
      pathLock.get(job.spec.path) === job.id
    ) {
      pathLock.delete(job.spec.path);
    }
    completed.set(job.id, job);
    while (completed.size > COMPLETED_CAP) {
      const oldest = completed.keys().next().value as string | undefined;
      if (oldest === undefined) break;
      completed.delete(oldest);
    }
  }

  function dispatch(spec: JobSpec, opts: DispatchOpts = {}): Promise<DispatchResult> {
    // #8 same-path write serialization (no queue).
    if (spec.isWrite && spec.path && pathLock.has(spec.path)) {
      return Promise.resolve({
        status: "BUSY" as const,
        jobId: pathLock.get(spec.path)!,
        busyPath: spec.path,
      });
    }

    const jobId = randomUUID();
    const handle = backend.run(spec);

    const job: Job = {
      id: jobId,
      status: "RUNNING",
      spec,
      child: handle.child,
      startedAt: clock.now(),
      lastTool: null,
      tokensSoFar: 0,
      lastAssistant: null,
      filesTouched: [],
      phase: null,
      lastEventAt: clock.now(),
      terminationReason: null,
      idleTimer: null,
      completion: Promise.resolve(null as unknown as RunOutput), // replaced below
      terminalOutput: null,
      sinks: new Set(),
    };
    active.set(jobId, job);
    if (spec.isWrite && spec.path) pathLock.set(spec.path, jobId);

    const armIdle = () => {
      if (idleMs === null) return;
      if (job.idleTimer) clock.clearTimer(job.idleTimer);
      job.idleTimer = clock.setTimer(idleMs, () => {
        if (job.status !== "RUNNING") return;
        job.terminationReason = "STALLED";
        job.child?.kill("SIGTERM");
      });
    };
    armIdle();

    const clearIdle = () => {
      if (job.idleTimer) {
        clock.clearTimer(job.idleTimer);
        job.idleTimer = null;
      }
    };
    handle.child?.on?.("close", clearIdle);
    handle.child?.on?.("error", clearIdle);

    handle.events.on("progress", (snap: ProgressSnapshotRaw) => {
      job.lastTool = snap.lastTool;
      job.tokensSoFar = snap.tokensSoFar;
      job.lastAssistant = snap.lastAssistant;
      job.filesTouched = snap.filesTouched;
      job.phase = snap.phase;
      job.lastEventAt = clock.now();
      armIdle();
      if (job.sinks.size > 0) {
        const u: ProgressUpdate = {
          lastTool: snap.lastTool,
          tokensSoFar: snap.tokensSoFar,
          elapsedMs: clock.now() - job.startedAt,
          phase: snap.phase ?? undefined,
        };
        for (const sink of job.sinks) sink({ ...u });
      }
    });
    handle.events.on("stderr", () => {
      job.lastEventAt = clock.now();
      armIdle();
    });

    const finalizeJob = async (res: BackendResult): Promise<RunOutput> => {
      clearIdle();

      // Cancel/stall: skip finalize entirely (gate side effects + git) — #6.
      if (job.terminationReason) {
        const out = baseOutput(res, {
          model: spec.model,
          backend: spec.backend,
          priceMap: spec.priceMap,
          jobId: job.id,
          downgraded: spec.downgraded,
        });
        retire(job, out);
        return out;
      }

      const ctx: FinalizeCtx = {
        cwd: spec.cwd,
        headBefore: spec.headBefore,
        isWrite: spec.isWrite,
        gate: spec.gate,
        allowPartialCommit: spec.allowPartialCommit,
        model: spec.model,
        backend: spec.backend,
        priceMap: spec.priceMap,
        jobId: job.id,
        downgraded: spec.downgraded,
        worktreeName: spec.worktreeName,
      };
      const out = await finalize(res, ctx);
      retire(job, out);
      return out;
    };

    const finalizeJobError = (err: unknown): RunOutput => {
      const out: RunOutput = {
        status: "ERROR",
        text: String((err as Error)?.message ?? err),
        sessionId: null,
        backend: spec.backend,
        model: spec.model,
        usage: null,
        costUsd: null,
        costEstimated: true,
        durationMs: null,
        jobId: job.id,
      };
      retire(job, out);
      return out;
    };

    job.completion = handle.done.then(finalizeJob, finalizeJobError);

    // background -> detach immediately, dropping the caller sink.
    if (spec.background) {
      return Promise.resolve({
        status: "RUNNING" as const,
        jobId,
      });
    }

    if (opts.sink) job.sinks.add(opts.sink);

    const deadline =
      spec.waitMs !== undefined ? clampWait(spec.waitMs) : deadlineMs;

    return raceDeadline(job, deadline, opts.signal).then((winner) => {
      if (opts.sink) job.sinks.delete(opts.sink);
      if (winner.kind === "done") return winner.out;
      return {
        status: "RUNNING" as const,
        jobId,
      };
    });
  }

  type RaceWinner = { kind: "done"; out: RunOutput } | { kind: "other" };

  function raceDeadline(
    job: Job,
    deadline: number,
    signal?: AbortSignal,
  ): Promise<RaceWinner> {
    return new Promise<RaceWinner>((resolve) => {
      let settled = false;
      let cleanupAbort = () => {};
      const finish = (v: RaceWinner) => {
        if (settled) return;
        settled = true;
        clock.clearTimer(timer);
        cleanupAbort();
        resolve(v);
      };
      const timer = clock.setTimer(deadline, () => finish({ kind: "other" }));
      void job.completion.then((out) => finish({ kind: "done", out }));
      if (signal) {
        if (signal.aborted) finish({ kind: "other" });
        else {
          const onAbort = () => finish({ kind: "other" });
          signal.addEventListener("abort", onAbort);
          cleanupAbort = () => signal.removeEventListener("abort", onAbort);
        }
      }
    });
  }

  function poll(jobId: string): PollResult {
    const job = getJob(jobId);
    if (!job) return { status: "NOT_FOUND" };
    if (job.status === "RUNNING") {
      return {
        status: "RUNNING",
        progress: {
          lastTool: job.lastTool,
          tokensSoFar: job.tokensSoFar,
          elapsedMs: clock.now() - job.startedAt,
          lastAssistant: job.lastAssistant,
          filesTouchedSoFar: job.filesTouched,
          phase: job.phase,
        },
      };
    }
    return {
      status: job.status as Exclude<JobStatus, "RUNNING">,
      result: job.terminalOutput!,
    };
  }

  async function cancel(jobId: string): Promise<PollResult> {
    const job = active.get(jobId);
    if (!job) return poll(jobId);
    job.terminationReason = "CANCELLED";
    job.child?.kill("SIGTERM");
    await job.completion.catch(() => {});
    return poll(jobId);
  }

  /** Race a set of completion promises vs a timeout vs abort. Resolves when the predicate settles. */
  function raceTimeout(
    completions: Promise<unknown>[],
    timeoutMs: number,
    mode: "any" | "all",
    signal?: AbortSignal,
  ): Promise<void> {
    return new Promise<void>((resolve) => {
      let settled = false;
      let cleanupAbort = () => {};
      const finish = () => {
        if (settled) return;
        settled = true;
        clock.clearTimer(timer);
        cleanupAbort();
        resolve();
      };
      const timer = clock.setTimer(timeoutMs, finish);
      if (completions.length === 0) {
        finish();
      } else if (mode === "any") {
        for (const c of completions) void c.then(finish, finish);
      } else {
        void Promise.all(completions.map((c) => c.catch(() => {}))).then(
          finish,
          finish,
        );
      }
      if (signal) {
        if (signal.aborted) finish();
        else {
          const onAbort = () => finish();
          signal.addEventListener("abort", onAbort);
          cleanupAbort = () => signal.removeEventListener("abort", onAbort);
        }
      }
    });
  }

  async function wait(
    jobId: string,
    timeoutMs = DEFAULT_WAIT_TIMEOUT,
    opts: WaitOpts = {},
  ): Promise<PollResult> {
    const job = active.get(jobId);
    if (!job) return poll(jobId);
    if (job.status !== "RUNNING") return poll(jobId);
    if (opts.sink) job.sinks.add(opts.sink);
    try {
      await raceTimeout([job.completion], clampWait(timeoutMs), "any", opts.signal);
    } finally {
      if (opts.sink) job.sinks.delete(opts.sink);
    }
    return poll(jobId);
  }

  function withTag(job: Job, sink: ProgressSink): ProgressSink {
    const tag = job.id.slice(0, 6);
    return (u: ProgressUpdate) => sink({ ...u, jobTag: tag });
  }

  async function waitAny(
    jobIds: string[],
    timeoutMs = DEFAULT_WAIT_TIMEOUT,
    opts: WaitOpts = {},
  ): Promise<{ jobs: Record<string, PollResult>; firstDone?: string }> {
    const known = jobIds
      .map((id) => ({ id, job: getJob(id) }))
      .filter((x): x is { id: string; job: Job } => !!x.job);

    let firstDone: string | undefined;
    const taggedSinks: Array<{ job: Job; sink: ProgressSink }> = [];

    const completions: Promise<unknown>[] = [];
    for (const { id, job } of known) {
      if (job.status !== "RUNNING") {
        if (firstDone === undefined) firstDone = id;
        continue;
      }
      if (opts.sink) {
        const tagged = withTag(job, opts.sink);
        job.sinks.add(tagged);
        taggedSinks.push({ job, sink: tagged });
      }
      completions.push(
        job.completion.then(() => {
          if (firstDone === undefined) firstDone = id;
        }),
      );
    }

    if (firstDone === undefined && completions.length > 0) {
      await raceTimeout(completions, clampWait(timeoutMs), "any", opts.signal);
    }
    for (const { job, sink } of taggedSinks) job.sinks.delete(sink);

    const jobs: Record<string, PollResult> = {};
    for (const { id } of known) jobs[id] = poll(id);
    return { jobs, firstDone };
  }

  async function waitAll(
    jobIds: string[],
    timeoutMs = DEFAULT_WAIT_TIMEOUT,
    opts: WaitOpts = {},
  ): Promise<{ jobs: Record<string, PollResult>; allDone: boolean }> {
    if (jobIds.length === 0) return { jobs: {}, allDone: true };

    const known = jobIds
      .map((id) => ({ id, job: getJob(id) }))
      .filter((x): x is { id: string; job: Job } => !!x.job);

    const taggedSinks: Array<{ job: Job; sink: ProgressSink }> = [];
    const completions: Promise<unknown>[] = [];
    for (const { job } of known) {
      if (job.status !== "RUNNING") continue;
      if (opts.sink) {
        const tagged = withTag(job, opts.sink);
        job.sinks.add(tagged);
        taggedSinks.push({ job, sink: tagged });
      }
      completions.push(job.completion);
    }

    if (completions.length > 0) {
      await raceTimeout(completions, clampWait(timeoutMs), "all", opts.signal);
    }
    for (const { job, sink } of taggedSinks) job.sinks.delete(sink);

    const jobs: Record<string, PollResult> = {};
    for (const { id } of known) jobs[id] = poll(id);
    // allDone recomputed from poll state; NOT_FOUND ids are not blocking (excluded from `known`).
    const allDone = known.every(({ id }) => poll(id).status !== "RUNNING");
    return { jobs, allDone };
  }

  function killAll(): void {
    for (const job of active.values()) {
      job.child?.kill("SIGTERM");
    }
  }

  return { dispatch, poll, cancel, wait, waitAny, waitAll, killAll };
}
