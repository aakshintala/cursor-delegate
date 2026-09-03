import { randomUUID } from "node:crypto";
import type { ChildProcess } from "node:child_process";
import type {
  DispatchResult,
  FinalizeCtx,
  JobSpec,
  JobStatus,
  PollResult,
  ResumeContext,
  RunOutput,
} from "./types.js";
import type { Backend, BackendResult, ProgressSnapshotRaw } from "./backends/types.js";
import type { ProgressSink, ProgressUpdate } from "./progress.js";
import {
  finalizeRun as defaultFinalize,
  finalizeStall as defaultFinalizeStall,
} from "./finalize.js";
import {
  fileStatusRecordWriter,
  type StatusRecordWriter,
} from "./status-record.js";
import { clampWait } from "./util.js";

export type { StatusRecordWriter } from "./status-record.js";
export { fileStatusRecordWriter } from "./status-record.js";

const COMPLETED_CAP = 100;
const DEFAULT_WAIT_TIMEOUT = 120000;
/** How often a RUNNING job's status record is refreshed on disk. */
const HEARTBEAT_MS = 30_000;

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
  heartbeatTimer: TimerHandle | null;
  supersededBy: string | null;
  completion: Promise<RunOutput>;
  terminalOutput: RunOutput | null;
  sinks: Set<ProgressSink>;
}

/**
 * A CANCELLED/STALLED job never gets a terminal `result` line, so there is no `text` to
 * report from the backend. Build one from whatever live progress the registry already
 * tracked, so the caller learns something concrete instead of an empty string.
 */
function describeStallProgress(job: Job, elapsedMs: number): string {
  const verb = job.terminationReason === "CANCELLED" ? "Cancelled" : "Idle watchdog killed this job";
  const parts = [`${verb} after ${Math.round(elapsedMs / 1000)}s.`];
  if (job.phase) parts.push(`Last phase: ${job.phase}.`);
  if (job.lastTool) parts.push(`Last tool: ${job.lastTool}.`);
  if (job.tokensSoFar > 0) parts.push(`${job.tokensSoFar} tokens streamed before the kill.`);
  if (job.lastAssistant) parts.push(`Last assistant text: "${job.lastAssistant}"`);
  if (job.filesTouched.length > 0) {
    parts.push(`Files touched: ${job.filesTouched.join(", ")}.`);
  }
  return parts.join(" ");
}

export interface DispatchOpts {
  sink?: ProgressSink;
  signal?: AbortSignal;
}

export interface WaitOpts {
  sink?: ProgressSink;
  signal?: AbortSignal;
}

export type AnswerLookup =
  | { ok: true; sessionId: string; resumeContext: ResumeContext }
  | { ok: false; error: "NOT_FOUND" }
  | { ok: false; error: "NOT_AWAITING"; status: JobStatus };

export interface JobRegistry {
  dispatch(spec: JobSpec, opts?: DispatchOpts): Promise<DispatchResult>;
  poll(jobId: string): PollResult;
  cancel(jobId: string): Promise<PollResult>;
  /** Record that `oldJobId` continues under `newJobId` (resume path) so watchers can follow the chain. */
  markSuperseded(oldJobId: string, newJobId: string): PollResult;
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
  lookupAnswer(jobId: string): AnswerLookup;
}

export interface RegistryDeps {
  backend: Backend;
  deadlineMs: number;
  idleMs: number | null;
  /** Wider idle window applied while a tool call is in flight (`job.phase === "running_tool"`). */
  toolIdleMs: number | null;
  clock?: Clock;
  finalize?: (res: BackendResult, ctx: FinalizeCtx) => Promise<RunOutput>;
  finalizeStall?: (res: BackendResult, ctx: FinalizeCtx) => Promise<RunOutput>;
  statusWriter?: StatusRecordWriter;
}

export function makeJobRegistry(deps: RegistryDeps): JobRegistry {
  const { backend, deadlineMs, idleMs, toolIdleMs } = deps;
  const clock = deps.clock ?? realClock;
  const finalize = deps.finalize ?? defaultFinalize;
  const finalizeStall = deps.finalizeStall ?? defaultFinalizeStall;
  const baseStatusWriter = deps.statusWriter ?? fileStatusRecordWriter();
  const statusWriter: StatusRecordWriter = {
    write(jobId, record) {
      try {
        baseStatusWriter.write(jobId, record);
      } catch {
        // conforming writers never throw; swallow misbehaving injectables too
      }
    },
  };

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
    if (job.heartbeatTimer) {
      clock.clearTimer(job.heartbeatTimer);
      job.heartbeatTimer = null;
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
    statusWriter.write(job.id, poll(job.id));
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
      heartbeatTimer: null,
      supersededBy: null,
      completion: Promise.resolve(null as unknown as RunOutput), // replaced below
      terminalOutput: null,
      sinks: new Set(),
    };
    active.set(jobId, job);
    statusWriter.write(jobId, poll(jobId));
    if (spec.isWrite && spec.path) pathLock.set(spec.path, jobId);

    // Per-call override wins when present (including explicit `null` to disable);
    // omitted falls back to the server-wide default.
    const effectiveIdleMs = spec.idleMs !== undefined ? spec.idleMs : idleMs;
    const effectiveToolIdleMs =
      spec.toolIdleMs !== undefined ? spec.toolIdleMs : toolIdleMs;
    // Tiered by phase (#9): a tool call in flight can go silent for a long time
    // legitimately (a build, a test suite) — that isn't a hang. No tool in flight and no
    // event arriving means we're waiting on the model itself, where a short silence really
    // is anomalous. Re-evaluated on every arm, since phase can change between events.
    const armIdle = () => {
      const window =
        job.phase === "running_tool" ? effectiveToolIdleMs : effectiveIdleMs;
      if (window === null) return;
      if (job.idleTimer) clock.clearTimer(job.idleTimer);
      job.idleTimer = clock.setTimer(window, () => {
        if (job.status !== "RUNNING") return;
        job.terminationReason = "STALLED";
        job.child?.kill("SIGTERM");
      });
    };
    armIdle();

    // Heartbeat: refresh the status record periodically so a long RUNNING job keeps its
    // progress current on disk and a frozen record (dead server) is detectable via
    // lastHeartbeatAt instead of looking like an active-but-idle job.
    const armHeartbeat = () => {
      job.heartbeatTimer = clock.setTimer(HEARTBEAT_MS, () => {
        if (job.status !== "RUNNING") return;
        statusWriter.write(job.id, poll(job.id));
        armHeartbeat();
      });
    };
    armHeartbeat();

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
    // Raw stdout bytes, even ones that don't complete a parseable line yet — cheap extra
    // liveness signal on top of the phase tiering, though it won't help when cursor-agent
    // buffers a tool's entire output until the tool finishes.
    handle.events.on("activity", () => {
      job.lastEventAt = clock.now();
      armIdle();
    });

    const finalizeJob = async (res: BackendResult): Promise<RunOutput> => {
      clearIdle();

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

      // Cancel/stall (#9): no gate (meaningless against a killed-mid-flight run) and no
      // incomplete-commit concern, but still compute the change-set — whatever the agent
      // already wrote before the kill is exactly what a caller needs to decide whether to
      // keep, discard, or redispatch — and surface the job's last known live progress as
      // `text`, since the backend never got a terminal `result` line to report from.
      if (job.terminationReason) {
        const out = await finalizeStall(res, ctx);
        out.text = describeStallProgress(job, clock.now() - job.startedAt);
        retire(job, out);
        return out;
      }

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
        lastHeartbeatAt: clock.now(),
        ...(job.supersededBy ? { supersededBy: job.supersededBy } : {}),
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
      ...(job.supersededBy ? { supersededBy: job.supersededBy } : {}),
    };
  }

  function markSuperseded(oldJobId: string, newJobId: string): PollResult {
    const job = getJob(oldJobId);
    if (!job) return { status: "NOT_FOUND" };
    job.supersededBy = newJobId;
    statusWriter.write(job.id, poll(job.id));
    return poll(job.id);
  }

  function lookupAnswer(jobId: string): AnswerLookup {
    const job = getJob(jobId);
    if (!job) return { ok: false, error: "NOT_FOUND" };
    const sessionId = job.terminalOutput?.sessionId ?? null;
    if (job.status !== "NEEDS_CONTEXT" || !sessionId) {
      return { ok: false, error: "NOT_AWAITING", status: job.status };
    }
    return {
      ok: true,
      sessionId,
      resumeContext: job.spec.resumeContext,
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

  return { dispatch, poll, cancel, markSuperseded, wait, waitAny, waitAll, killAll, lookupAnswer };
}
