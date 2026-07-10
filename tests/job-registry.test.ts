import { test } from "node:test";
import assert from "node:assert/strict";
import { makeJobRegistry, type RegistryDeps } from "../src/job-registry.js";
import type { BackendResult } from "../src/backends/types.js";
import type { DispatchResult, RunOutput } from "../src/types.js";
import {
  FakeClock,
  fakeFinalize,
  flush,
  makeFakeBackend,
  specOf,
} from "./helpers.js";

const FINALIZE = fakeFinalize as unknown as RegistryDeps["finalize"];

const doneOk: BackendResult = {
  raw: { result: "ok\nSTATUS: DONE", is_error: false },
  cleanExit: true,
  stderr: "",
};

function setup(over: Partial<RegistryDeps> = {}) {
  const { backend, handles } = makeFakeBackend();
  const clock = new FakeClock();
  const registry = makeJobRegistry({
    backend,
    deadlineMs: 1000,
    idleMs: null,
    clock,
    finalize: FINALIZE,
    ...over,
  });
  return { registry, handles, clock };
}

function jobId(r: DispatchResult): string {
  return (r as { jobId: string }).jobId;
}

test("a task finishing within the deadline returns a RunOutput", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf());
  handles[0].finish(doneOk);
  const res = (await p) as RunOutput;
  assert.equal(res.status, "DONE");
  assert.equal(res.text, "ok\nSTATUS: DONE");
});

test("a task exceeding the deadline returns {RUNNING, jobId}, later pollable to terminal", async () => {
  const { registry, handles, clock } = setup();
  const p = registry.dispatch(specOf());
  await clock.advance(1000); // blow the deadline
  const res = await p;
  assert.equal(res.status, "RUNNING");
  const id = jobId(res);

  // The child is NOT killed by the deadline.
  assert.deepEqual(handles[0].child.killed, []);

  handles[0].finish(doneOk);
  await flush();
  const poll = registry.poll(id);
  assert.equal(poll.status, "DONE");
  if (poll.status !== "RUNNING" && poll.status !== "NOT_FOUND") {
    assert.equal(poll.result.status, "DONE");
  }
});

test("background returns immediately without blocking", async () => {
  const { registry } = setup();
  const res = await registry.dispatch(specOf({ background: true }));
  assert.equal(res.status, "RUNNING");
});

test("a second write to a locked CallerProvided path returns BUSY", async () => {
  const { registry } = setup();
  const s = () => specOf({ isWrite: true, path: "/repo", background: true });
  const r1 = await registry.dispatch(s());
  assert.equal(r1.status, "RUNNING");
  const r2 = await registry.dispatch(s());
  assert.equal(r2.status, "BUSY");
  assert.equal((r2 as { busyPath: string }).busyPath, "/repo");
});

test("the lock releases when the holder finishes", async () => {
  const { registry, handles } = setup();
  const s = () => specOf({ isWrite: true, path: "/repo", background: true });
  await registry.dispatch(s());
  handles[0].finish(doneOk);
  await flush();
  const r2 = await registry.dispatch(s());
  assert.equal(r2.status, "RUNNING"); // no longer BUSY
});

test("cancel SIGTERMs the child and marks the job CANCELLED", async () => {
  const { registry, handles } = setup();
  const r = await registry.dispatch(specOf({ background: true }));
  const poll = await registry.cancel(jobId(r));
  assert.equal(poll.status, "CANCELLED");
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
});

test("the idle watchdog SIGTERMs a silent job and marks it STALLED", async () => {
  const { registry, handles, clock } = setup({ idleMs: 5000, deadlineMs: 100000 });
  const r = await registry.dispatch(specOf({ background: true }));
  await clock.advance(5000); // no events arrived
  await flush();
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
  assert.equal(registry.poll(jobId(r)).status, "STALLED");
});

test("a progress event re-arms the idle watchdog (no premature STALLED)", async () => {
  const { registry, handles, clock } = setup({ idleMs: 5000, deadlineMs: 100000 });
  const r = await registry.dispatch(specOf({ background: true }));
  await clock.advance(4000);
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 1,
    lastAssistant: null,
    filesTouched: [],
    phase: null,
  });
  await clock.advance(4000); // 8000 total, but only 4000 since the event
  assert.deepEqual(handles[0].child.killed, []);
  assert.equal(registry.poll(jobId(r)).status, "RUNNING");
});

test("poll on an unknown id is NOT_FOUND", () => {
  const { registry } = setup();
  assert.equal(registry.poll("nope").status, "NOT_FOUND");
});

test("wait resolves when the job completes", async () => {
  const { registry, handles } = setup({ deadlineMs: 100000 });
  const r = await registry.dispatch(specOf({ background: true }));
  const wp = registry.wait(jobId(r), 10000);
  handles[0].finish(doneOk);
  const poll = await wp;
  assert.equal(poll.status, "DONE");
});

test("wait returns the RUNNING snapshot on timeout", async () => {
  const { registry, clock } = setup({ deadlineMs: 100000 });
  const r = await registry.dispatch(specOf({ background: true }));
  const wp = registry.wait(jobId(r), 3000);
  await clock.advance(3000);
  const poll = await wp;
  assert.equal(poll.status, "RUNNING");
});

test("waitAny resolves on the first terminal job", async () => {
  const { registry, handles } = setup({ deadlineMs: 100000 });
  const a = await registry.dispatch(specOf({ background: true }));
  const b = await registry.dispatch(specOf({ background: true }));
  const wp = registry.waitAny([jobId(a), jobId(b)], 10000);
  handles[1].finish(doneOk);
  const res = await wp;
  assert.equal(res.firstDone, jobId(b));
  assert.equal(res.jobs[jobId(b)].status, "DONE");
  assert.equal(res.jobs[jobId(a)].status, "RUNNING");
});

test("waitAll resolves when all known jobs are terminal", async () => {
  const { registry, handles } = setup({ deadlineMs: 100000 });
  const a = await registry.dispatch(specOf({ background: true }));
  const b = await registry.dispatch(specOf({ background: true }));
  const wp = registry.waitAll([jobId(a), jobId(b)], 10000);
  handles[0].finish(doneOk);
  handles[1].finish(doneOk);
  const res = await wp;
  assert.equal(res.allDone, true);
  assert.equal(res.jobs[jobId(a)].status, "DONE");
  assert.equal(res.jobs[jobId(b)].status, "DONE");
});

test("waitAll with empty input is immediately allDone", async () => {
  const { registry } = setup();
  const res = await registry.waitAll([], 1000);
  assert.deepEqual(res, { jobs: {}, allDone: true });
});

test("killAll SIGTERMs every active child", async () => {
  const { registry, handles } = setup({ deadlineMs: 100000 });
  await registry.dispatch(specOf({ background: true }));
  await registry.dispatch(specOf({ background: true }));
  registry.killAll();
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
  assert.deepEqual(handles[1].child.killed, ["SIGTERM"]);
});

test("idle timer cleared at child close does not STALL during a slow finalize", async () => {
  let unblock!: () => void;
  const slowFinalize = async (
    res: BackendResult,
    ctx: { backend: string; model: string; jobId?: string },
  ) => {
    await new Promise<void>((r) => {
      unblock = r;
    });
    return fakeFinalize(res, ctx);
  };
  const { registry, handles, clock } = setup({
    idleMs: 50,
    deadlineMs: 100000,
    finalize: slowFinalize as unknown as RegistryDeps["finalize"],
  });
  const r = await registry.dispatch(specOf({ background: true }));
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 1,
    lastAssistant: null,
    filesTouched: [],
    phase: null,
  });
  handles[0].finish(doneOk);
  await clock.advance(100);
  assert.equal(registry.poll(jobId(r)).status, "RUNNING");
  unblock();
  await flush();
  assert.equal(registry.poll(jobId(r)).status, "DONE");
});

test("cancel skips finalize side effects", async () => {
  let gateRan = false;
  const trackingFinalize = async (
    res: BackendResult,
    ctx: { gate?: string; runGate?: unknown },
  ) => {
    if (ctx.gate) gateRan = true;
    return fakeFinalize(res, ctx as { backend: string; model: string });
  };
  const { registry, handles } = setup({
    finalize: trackingFinalize as unknown as RegistryDeps["finalize"],
  });
  const r = await registry.dispatch(specOf({ background: true, gate: "make test" }));
  await registry.cancel(jobId(r));
  assert.equal(gateRan, false);
  assert.equal(registry.poll(jobId(r)).status, "CANCELLED");
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
});

const needsContextOk: BackendResult = {
  raw: {
    result: "Which API version should I target?\nSTATUS: NEEDS_CONTEXT",
    is_error: false,
    session_id: "sess-park-1",
  },
  cleanExit: true,
  stderr: "",
};

test("foreground NEEDS_CONTEXT returns RunOutput with jobId and parks the job", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf());
  handles[0].finish(needsContextOk);
  const res = (await p) as RunOutput;
  assert.equal(res.status, "NEEDS_CONTEXT");
  assert.ok(typeof res.jobId === "string" && res.jobId.length > 0);
  assert.equal(res.sessionId, "sess-park-1");
  assert.match(res.text, /Which API version/);

  const looked = registry.lookupAnswer(res.jobId!);
  assert.equal(looked.ok, true);
  if (looked.ok) {
    assert.equal(looked.sessionId, "sess-park-1");
    assert.equal(looked.resumeContext.model, "composer-2.5");
  }
});

test("lookupAnswer returns NOT_FOUND for unknown or expired ids", () => {
  const { registry } = setup();
  assert.deepEqual(registry.lookupAnswer("nope"), {
    ok: false,
    error: "NOT_FOUND",
  });
});

test("lookupAnswer rejects jobs that are not awaiting an answer", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf({ background: true }));
  const id = jobId(await p);
  assert.deepEqual(registry.lookupAnswer(id), {
    ok: false,
    error: "NOT_AWAITING",
    status: "RUNNING",
  });

  handles[0].finish(doneOk);
  await flush();
  assert.deepEqual(registry.lookupAnswer(id), {
    ok: false,
    error: "NOT_AWAITING",
    status: "DONE",
  });
});

test("lookupAnswer rejects NEEDS_CONTEXT jobs that lack a sessionId", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf());
  handles[0].finish({
    raw: {
      result: "Need a choice\nSTATUS: NEEDS_CONTEXT",
      is_error: false,
    },
    cleanExit: true,
    stderr: "",
  });
  const res = (await p) as RunOutput;
  assert.equal(res.status, "NEEDS_CONTEXT");
  assert.deepEqual(registry.lookupAnswer(res.jobId!), {
    ok: false,
    error: "NOT_AWAITING",
    status: "NEEDS_CONTEXT",
  });
});
