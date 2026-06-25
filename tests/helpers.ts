import { EventEmitter } from "node:events";
import type { Backend, BackendResult } from "../src/backends/types.js";
import type { Clock, TimerHandle } from "../src/job-registry.js";
import type { JobSpec } from "../src/types.js";

/** Flush pending microtasks/immediates. */
export const flush = (): Promise<void> =>
  new Promise((r) => setImmediate(r));

/** Deterministic clock implementing the registry's Clock interface. */
export class FakeClock implements Clock {
  t = 0;
  private timers = new Map<
    number,
    { at: number; cb: () => void; cleared: boolean }
  >();
  private seq = 0;

  now(): number {
    return this.t;
  }
  setTimer(ms: number, cb: () => void): TimerHandle {
    const id = ++this.seq;
    this.timers.set(id, { at: this.t + ms, cb, cleared: false });
    return id;
  }
  clearTimer(h: TimerHandle): void {
    const e = this.timers.get(h as number);
    if (e) e.cleared = true;
  }
  /** Advance time and fire any due timers, then flush microtasks. */
  async advance(ms: number): Promise<void> {
    this.t += ms;
    for (const [id, e] of [...this.timers]) {
      if (!e.cleared && e.at <= this.t) {
        e.cleared = true;
        this.timers.delete(id);
        e.cb();
      }
    }
    await flush();
  }
}

export interface FakeChild {
  killed: string[];
  kill(sig?: string): void;
}

export interface FakeHandle {
  spec: JobSpec;
  child: FakeChild;
  events: EventEmitter;
  emitProgress(snap: Record<string, unknown>): void;
  emitStderr(s: string): void;
  finish(r: BackendResult): void;
  /** True once `done` has resolved. */
  done: Promise<BackendResult>;
}

/** Controllable fake backend: each run() pushes a handle the test drives. */
export function makeFakeBackend(): { backend: Backend; handles: FakeHandle[] } {
  const handles: FakeHandle[] = [];
  const backend: Backend = {
    run(spec: JobSpec) {
      const events = new EventEmitter();
      let resolveDone!: (r: BackendResult) => void;
      const done = new Promise<BackendResult>((r) => {
        resolveDone = r;
      });
      const child: FakeChild = {
        killed: [],
        kill(sig?: string) {
          child.killed.push(sig ?? "SIGTERM");
          // Simulate the child dying after a tick (non-clean exit).
          queueMicrotask(() =>
            resolveDone({ raw: {}, cleanExit: false, stderr: "" }),
          );
        },
      };
      const handle: FakeHandle = {
        spec,
        child,
        events,
        emitProgress: (snap) => events.emit("progress", snap),
        emitStderr: (s) => events.emit("stderr", s),
        finish: (r) => resolveDone(r),
        done,
      };
      handles.push(handle);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return { child: child as any, events, done };
    },
  };
  return { backend, handles };
}

/** A finalize that builds a RunOutput from the raw result without touching git/gate. */
export const fakeFinalize = async (
  res: BackendResult,
  ctx: { backend: string; model: string; jobId?: string },
) => ({
  status:
    res.cleanExit && res.raw.is_error === false
      ? ("DONE" as const)
      : ("ERROR" as const),
  text: res.raw.result ?? "",
  sessionId: res.raw.session_id ?? null,
  backend: ctx.backend,
  model: ctx.model,
  usage: res.raw.usage ?? null,
  costUsd: null,
  costEstimated: true,
  durationMs: res.raw.duration_ms ?? null,
  jobId: ctx.jobId,
});

/** Minimal JobSpec factory for registry tests. */
export function specOf(over: Partial<JobSpec> = {}): JobSpec {
  return {
    bin: "cursor-agent",
    argv: ["--print"],
    cwd: "/tmp",
    model: "composer-2.5",
    backend: "cursor",
    isWrite: false,
    path: null,
    headBefore: null,
    gate: "",
    allowPartialCommit: false,
    priceMap: {},
    downgraded: false,
    ...over,
  };
}
