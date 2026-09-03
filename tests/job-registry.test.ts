import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  fileStatusRecordWriter,
  makeJobRegistry,
  type RegistryDeps,
  type StatusRecordWriter,
} from "../src/job-registry.js";
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

const noopStatusWriter: StatusRecordWriter = { write: () => {} };

function setup(over: Partial<RegistryDeps> = {}) {
  const { backend, handles } = makeFakeBackend();
  const clock = new FakeClock();
  const registry = makeJobRegistry({
    backend,
    deadlineMs: 1000,
    idleMs: null,
    toolIdleMs: null,
    clock,
    finalize: FINALIZE,
    finalizeStall: FINALIZE,
    statusWriter: noopStatusWriter,
    ...over,
  });
  return { registry, handles, clock };
}

function statusSpy(): {
  writer: StatusRecordWriter;
  writes: Array<{ jobId: string; record: unknown }>;
} {
  const writes: Array<{ jobId: string; record: unknown }> = [];
  return {
    writes,
    writer: {
      write(jobId, record) {
        writes.push({ jobId, record });
      },
    },
  };
}

test("heartbeat refreshes the RUNNING record every 30s and stops at retirement", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles, clock } = setup({ statusWriter: writer });
  await registry.dispatch(specOf({ background: true }));
  assert.equal(writes.length, 1);

  await clock.advance(30_000);
  await flush();
  assert.equal(writes.length, 2);
  assert.equal(writes[1].record.status, "RUNNING");
  // Disk record and cursor_poll are the same shape by construction: every write is
  // poll(jobId) verbatim. This assert pins that so they can't drift apart.
  assert.deepEqual(writes[1].record, registry.poll(writes[1].jobId));
  if (writes[1].record.status === "RUNNING") {
    assert.equal(writes[1].record.lastHeartbeatAt, 30_000);
  }

  // Re-arms while RUNNING...
  await clock.advance(30_000);
  await flush();
  assert.equal(writes.length, 3);

  // ...and stops once terminal.
  handles[0].finish(doneOk);
  await flush();
  const afterTerminal = writes.length;
  assert.equal(writes[afterTerminal - 1].record.status, "DONE");
  await clock.advance(120_000);
  await flush();
  assert.equal(writes.length, afterTerminal);
});

test("markSuperseded writes a forwarding pointer into the old record", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles } = setup({ statusWriter: writer });
  const r = await registry.dispatch(specOf({ background: true }));
  const oldId = jobId(r);
  handles[0].finish({
    ...doneOk,
    raw: { result: "what port?\nSTATUS: NEEDS_CONTEXT", is_error: false },
    cleanExit: true,
  });
  await flush();
  assert.equal(registry.poll(oldId).status, "NEEDS_CONTEXT");

  const newId = randomUUID();
  const res = registry.markSuperseded(oldId, newId);
  assert.equal(res.status, "NEEDS_CONTEXT");
  if (res.status !== "RUNNING" && res.status !== "NOT_FOUND") {
    assert.equal(res.supersededBy, newId);
  }
  const last = writes[writes.length - 1];
  assert.equal(last.jobId, oldId);
  if (last.record.status !== "RUNNING" && last.record.status !== "NOT_FOUND") {
    assert.equal(last.record.supersededBy, newId);
  }
});

test("markSuperseded on an unknown job returns NOT_FOUND", () => {
  const { registry } = setup();
  assert.deepEqual(registry.markSuperseded(randomUUID(), randomUUID()), {
    status: "NOT_FOUND",
  });
});

function jobId(r: DispatchResult): string {
  return (r as { jobId: string }).jobId;
}

test("status persistence writes RUNNING at dispatch and terminal at completion", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles } = setup({ statusWriter: writer });
  const p = registry.dispatch(specOf());
  assert.equal(writes.length, 1);
  const id = writes[0].jobId;
  assert.deepEqual(writes[0].record, {
    status: "RUNNING",
    lastHeartbeatAt: 0,
    progress: {
      lastTool: null,
      tokensSoFar: 0,
      elapsedMs: 0,
      lastAssistant: null,
      filesTouchedSoFar: [],
      phase: null,
    },
  });

  handles[0].finish(doneOk);
  await flush();
  const res = (await p) as RunOutput;
  assert.equal(writes.length, 2);
  assert.deepEqual(writes[1].record, registry.poll(id));
  assert.equal(writes[1].record.status, "DONE");
  if (writes[1].record.status !== "RUNNING" && writes[1].record.status !== "NOT_FOUND") {
    assert.equal(writes[1].record.result.status, "DONE");
    assert.equal(writes[1].record.result.text, res.text);
  }
});

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

test("background dispatch persists start and terminal records", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles } = setup({ statusWriter: writer });
  const r = await registry.dispatch(specOf({ background: true }));
  const id = jobId(r);
  assert.equal(writes.length, 1);
  handles[0].finish(doneOk);
  await flush();
  assert.equal(writes.length, 2);
  assert.equal(writes[1].record.status, "DONE");
});

test("idle watchdog persistence writes STALLED terminal record", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles, clock } = setup({
    statusWriter: writer,
    idleMs: 5000,
    deadlineMs: 100000,
  });
  await registry.dispatch(specOf({ background: true }));
  assert.equal(writes.length, 1);
  await clock.advance(5000);
  await flush();
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
  assert.equal(writes.length, 2);
  assert.equal(writes[1].record.status, "STALLED");
});

test("cancel persists CANCELLED terminal record", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles } = setup({ statusWriter: writer });
  const r = await registry.dispatch(specOf({ background: true }));
  assert.equal(writes.length, 1);
  await registry.cancel(jobId(r));
  assert.equal(writes.length, 2);
  assert.equal(writes[1].record.status, "CANCELLED");
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
});

test("BUSY dispatch writes no status record", async () => {
  const { writes, writer } = statusSpy();
  const { registry } = setup({ statusWriter: writer });
  const s = () => specOf({ isWrite: true, path: "/repo", background: true });
  await registry.dispatch(s());
  assert.equal(writes.length, 1);
  const before = writes.length;
  const r2 = await registry.dispatch(s());
  assert.equal(r2.status, "BUSY");
  assert.equal(writes.length, before);
});

test("a throwing status writer does not affect job completion", async () => {
  let calls = 0;
  const statusWriter: StatusRecordWriter = {
    write() {
      calls++;
      if (calls === 2) throw new Error("boom");
    },
  };
  const { registry, handles } = setup({ statusWriter });
  const p = registry.dispatch(specOf());
  handles[0].finish(doneOk);
  const res = (await p) as RunOutput;
  assert.equal(res.status, "DONE");
  assert.equal(res.text, "ok\nSTATUS: DONE");
  assert.equal(calls, 2);
});

test("intermediate progress events do not trigger status writes", async () => {
  const { writes, writer } = statusSpy();
  const { registry, handles } = setup({ statusWriter: writer });
  await registry.dispatch(specOf({ background: true }));
  assert.equal(writes.length, 1);
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 42,
    lastAssistant: "running the test suite now",
    filesTouched: ["src/foo.rs"],
    phase: "running_tool",
  });
  assert.equal(writes.length, 1);
  handles[0].finish(doneOk);
  await flush();
  assert.equal(writes.length, 2);
});

test("file status record is overwritten from RUNNING to terminal", async () => {
  const writer = fileStatusRecordWriter();
  const { registry, handles } = setup({ statusWriter: writer });
  const r = await registry.dispatch(specOf({ background: true }));
  const id = jobId(r);
  const filePath = path.join(os.tmpdir(), "cursor-delegate-jobs", `${id}.json`);
  const running = JSON.parse(fs.readFileSync(filePath, "utf8"));
  assert.equal(running.status, "RUNNING");

  handles[0].finish(doneOk);
  await flush();
  const terminal = JSON.parse(fs.readFileSync(filePath, "utf8"));
  assert.equal(terminal.status, "DONE");
  const polled = registry.poll(id);
  if (polled.status !== "RUNNING" && polled.status !== "NOT_FOUND") {
    assert.deepEqual(terminal.result, polled.result);
  }
  fs.unlinkSync(filePath);
});

test("fileStatusRecordWriter writes independent per-job files", () => {
  const dir = path.join(os.tmpdir(), "cursor-delegate-jobs");
  const writer = fileStatusRecordWriter();
  const idA = randomUUID();
  const idB = randomUUID();
  const recordA = {
    status: "RUNNING" as const,
    progress: {
      lastTool: null,
      tokensSoFar: 0,
      elapsedMs: 0,
      lastAssistant: null,
      filesTouchedSoFar: [] as string[],
      phase: null,
    },
  };
  const recordB = {
    status: "DONE" as const,
    result: {
      status: "DONE" as const,
      text: "done",
      sessionId: null,
      backend: "cursor",
      model: "composer-2.5",
      usage: null,
      costUsd: null,
      costEstimated: true,
      durationMs: null,
      jobId: idB,
    },
  };
  const cwd = process.cwd();
  try {
    process.chdir(os.homedir());
    writer.write(idA, recordA);
    writer.write(idB, recordB);
    const pathA = path.join(dir, `${idA}.json`);
    const pathB = path.join(dir, `${idB}.json`);
    assert.deepEqual(JSON.parse(fs.readFileSync(pathA, "utf8")), recordA);
    assert.deepEqual(JSON.parse(fs.readFileSync(pathB, "utf8")), recordB);

    const updated = {
      ...recordA,
      progress: { ...recordA.progress, tokensSoFar: 99 },
    };
    writer.write(idA, updated);
    assert.deepEqual(JSON.parse(fs.readFileSync(pathA, "utf8")), updated);
  } finally {
    process.chdir(cwd);
    for (const id of [idA, idB]) {
      try {
        fs.unlinkSync(path.join(dir, `${id}.json`));
      } catch {
        // ignore cleanup failures
      }
    }
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

test("a STALLED job's text summarizes last known progress instead of being empty (#9)", async () => {
  const { registry, handles, clock } = setup({ idleMs: 5000, deadlineMs: 100000 });
  const p = registry.dispatch(specOf());
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 42,
    lastAssistant: "running the test suite now",
    filesTouched: ["src/foo.rs"],
    phase: "thinking", // not "running_tool" — keep this on the short idleMs window
  });
  await clock.advance(5000);
  await flush();
  const res = (await p) as RunOutput;
  assert.match(res.text, /shell/);
  assert.match(res.text, /42 tokens/);
  assert.match(res.text, /src\/foo\.rs/);
  assert.match(res.text, /running the test suite now/);
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

test("a per-call idleMs override takes priority over the server default", async () => {
  const { registry, handles, clock } = setup({ idleMs: 5000, deadlineMs: 100000 });
  const r = await registry.dispatch(
    specOf({ background: true, idleMs: 20000 }),
  );
  await clock.advance(5000); // would have STALLED under the 5000ms server default
  assert.deepEqual(handles[0].child.killed, []);
  assert.equal(registry.poll(jobId(r)).status, "RUNNING");
  await clock.advance(15000); // 20000 total, now past the per-call override
  await flush();
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
  assert.equal(registry.poll(jobId(r)).status, "STALLED");
});

test("a per-call idleMs: null disables the watchdog for that job", async () => {
  const { registry, handles, clock } = setup({ idleMs: 5000, deadlineMs: 100000 });
  const r = await registry.dispatch(
    specOf({ background: true, idleMs: null }),
  );
  await clock.advance(1_000_000); // would have STALLED many times over otherwise
  assert.deepEqual(handles[0].child.killed, []);
  assert.equal(registry.poll(jobId(r)).status, "RUNNING");
});

test("a tool call in flight uses toolIdleMs, not idleMs, and survives past the short window", async () => {
  const { registry, handles, clock } = setup({
    idleMs: 5000,
    toolIdleMs: 60000,
    deadlineMs: 1000000,
  });
  const r = await registry.dispatch(specOf({ background: true }));
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 1,
    lastAssistant: null,
    filesTouched: [],
    phase: "running_tool",
  });
  await clock.advance(5000); // past idleMs, but a tool is in flight
  assert.deepEqual(handles[0].child.killed, []);
  assert.equal(registry.poll(jobId(r)).status, "RUNNING");
  await clock.advance(55000); // 60000 total since the tool started — now past toolIdleMs
  await flush();
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
  assert.equal(registry.poll(jobId(r)).status, "STALLED");
});

test("leaving the tool phase (e.g. the model responds) reverts to the short idleMs window", async () => {
  const { registry, handles, clock } = setup({
    idleMs: 5000,
    toolIdleMs: 60000,
    deadlineMs: 1000000,
  });
  const r = await registry.dispatch(specOf({ background: true }));
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 1,
    lastAssistant: null,
    filesTouched: [],
    phase: "running_tool",
  });
  await clock.advance(30000); // fine — still under toolIdleMs
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 2,
    lastAssistant: "done with that, thinking about next step",
    filesTouched: [],
    phase: "responding",
  });
  await clock.advance(5000); // past the short idleMs, now that no tool is in flight
  await flush();
  assert.deepEqual(handles[0].child.killed, ["SIGTERM"]);
  assert.equal(registry.poll(jobId(r)).status, "STALLED");
});

test("a per-call toolIdleMs override applies only while a tool is in flight", async () => {
  const { registry, handles, clock } = setup({
    idleMs: 5000,
    toolIdleMs: 60000,
    deadlineMs: 1000000,
  });
  const r = await registry.dispatch(
    specOf({ background: true, toolIdleMs: 500000 }),
  );
  handles[0].emitProgress({
    lastTool: "shell",
    tokensSoFar: 1,
    lastAssistant: null,
    filesTouched: [],
    phase: "running_tool",
  });
  await clock.advance(60000); // past the server toolIdleMs, under the per-call override
  assert.deepEqual(handles[0].child.killed, []);
  assert.equal(registry.poll(jobId(r)).status, "RUNNING");
});

test("raw stdout activity re-arms the watchdog even without a fully parsed line", async () => {
  const { registry, handles, clock } = setup({ idleMs: 5000, deadlineMs: 100000 });
  const r = await registry.dispatch(specOf({ background: true }));
  await clock.advance(4000);
  handles[0].emitActivity();
  await clock.advance(4000); // 8000 total, but only 4000 since the activity event
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

test("completed LRU evicts past 100 jobs; evicted ids read NOT_FOUND", async () => {
  const { registry, handles } = setup();
  const ids: string[] = [];
  for (let i = 0; i < 101; i++) {
    const r = await registry.dispatch(specOf({ background: true }));
    ids.push(jobId(r));
  }
  assert.equal(new Set(ids).size, 101); // UUID ids are unique
  for (const h of handles) h.finish(doneOk);
  await flush();
  await flush();
  assert.equal(registry.poll(ids[0]).status, "NOT_FOUND");
  assert.equal(registry.poll(ids[100]).status, "DONE");
});

test("detaching a write returns busyPath naming the locked tree", async () => {
  const { registry, clock } = setup();
  const p = registry.dispatch(specOf({ isWrite: true, path: "/repo" }));
  await clock.advance(1000); // blow the deadline -> detach
  const detached = await p;
  assert.equal(detached.status, "RUNNING");
  assert.equal((detached as { busyPath?: string }).busyPath, "/repo");

  const bg = await registry.dispatch(
    specOf({ isWrite: true, path: "/other", background: true }),
  );
  assert.equal(bg.status, "RUNNING");
  assert.equal((bg as { busyPath?: string }).busyPath, "/other");
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

test("cancel during finalizing aborts the finalize stage instead of waiting forever", async () => {
  // Finalize hangs until ctx.signal aborts — models runGate's abort support.
  let signalSeen: AbortSignal | null = null;
  const hangingFinalize: RegistryDeps["finalize"] = async (res, ctx) => {
    signalSeen = ctx.signal ?? null;
    return new Promise((resolve) => {
      ctx.signal?.addEventListener("abort", () => {
        resolve({
          status: "ERROR",
          text: "finalize aborted",
          sessionId: null,
          backend: ctx.backend,
          model: ctx.model,
          usage: null,
          costUsd: null,
          costEstimated: true,
          durationMs: null,
          jobId: ctx.jobId,
        });
      });
    });
  };
  const { registry, handles } = setup({ finalize: hangingFinalize });
  const result = await registry.dispatch(specOf({ background: true }));
  assert.equal(result.status, "RUNNING");
  handles[0].finish(doneOk);
  // Wait until finalize has actually begun (signal captured).
  await new Promise<void>((res) => {
    const t = setInterval(() => {
      if (signalSeen !== null) { clearInterval(t); res(); }
    }, 5);
  });
  await registry.cancel(result.jobId);
  assert.equal(registry.poll(result.jobId).status, "CANCELLED");
});
