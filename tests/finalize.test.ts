import { test } from "node:test";
import assert from "node:assert/strict";
import { finalizeRun, finalizeStall } from "../src/finalize.js";
import type { BackendResult } from "../src/backends/types.js";
import type { ChangeSet, FinalizeCtx, GateResult } from "../src/types.js";

function baseCtx(over: Partial<FinalizeCtx> = {}): FinalizeCtx {
  return {
    cwd: "/repo",
    headBefore: null,
    isWrite: false,
    gate: "",
    allowPartialCommit: false,
    model: "composer-2.5",
    backend: "cursor",
    priceMap: {},
    gitDelta: async () => null, // not a repo by default
    ...over,
  };
}

const okResult: BackendResult = {
  raw: { result: "done\nSTATUS: DONE", is_error: false },
  cleanExit: true,
  stderr: "",
};

test("stderrTail is absent on a clean DONE", async () => {
  const out = await finalizeRun(
    { ...okResult, stderr: "some noise" },
    baseCtx(),
  );
  assert.equal(out.status, "DONE");
  assert.equal(out.stderrTail, undefined);
});

test("stderrTail is present on a non-clean exit", async () => {
  const out = await finalizeRun(
    { raw: { result: "" }, cleanExit: false, stderr: "boom" },
    baseCtx(),
  );
  assert.equal(out.status, "ERROR");
  assert.equal(out.stderrTail, "boom");
});

test("a failing gate downgrades DONE to DONE_WITH_CONCERNS", async () => {
  const gate: GateResult = {
    command: "make test",
    exitCode: 1,
    passed: false,
    outputTail: "FAIL",
  };
  const out = await finalizeRun(
    okResult,
    baseCtx({ gate: "make test", runGate: async () => gate }),
  );
  assert.equal(out.status, "DONE_WITH_CONCERNS");
  assert.deepEqual(out.gateResult, gate);
});

test("a passing gate leaves DONE intact", async () => {
  const out = await finalizeRun(
    okResult,
    baseCtx({
      gate: "make test",
      runGate: async () => ({
        command: "make test",
        exitCode: 0,
        passed: true,
        outputTail: "",
      }),
    }),
  );
  assert.equal(out.status, "DONE");
});

const dirtyCommitted: ChangeSet = {
  headBefore: "aaa",
  headAfter: "bbb",
  newCommits: ["bbb"],
  filesChanged: ["x"],
  diffstat: "x | 1 +",
  uncommittedFiles: ["y"],
  dirtyAfter: true,
};

test("incomplete-commit downgrades and adds a concern for a write", async () => {
  const out = await finalizeRun(
    okResult,
    baseCtx({ isWrite: true, headBefore: "aaa", gitDelta: async () => dirtyCommitted }),
  );
  assert.equal(out.status, "DONE_WITH_CONCERNS");
  assert.ok(out.concerns && out.concerns.length === 1);
  assert.deepEqual(out.changeSet, dirtyCommitted);
});

test("allowPartialCommit suppresses the incomplete-commit concern", async () => {
  const out = await finalizeRun(
    okResult,
    baseCtx({
      isWrite: true,
      allowPartialCommit: true,
      headBefore: "aaa",
      gitDelta: async () => dirtyCommitted,
    }),
  );
  assert.equal(out.status, "DONE");
  assert.equal(out.concerns, undefined);
});

test("jobId and downgraded flags propagate", async () => {
  const out = await finalizeRun(
    okResult,
    baseCtx({ jobId: "job-1", downgraded: true }),
  );
  assert.equal(out.jobId, "job-1");
  assert.equal(out.downgraded, true);
});

const stalledResult: BackendResult = {
  raw: {}, // killed before any `result` line arrived — nothing to read a status/text from
  cleanExit: false,
  stderr: "",
};

test("finalizeStall still computes the change-set (#9)", async () => {
  const out = await finalizeStall(
    stalledResult,
    baseCtx({ isWrite: true, headBefore: "aaa", gitDelta: async () => dirtyCommitted }),
  );
  assert.deepEqual(out.changeSet, dirtyCommitted);
});

test("finalizeStall never runs the gate, even when one is configured", async () => {
  let ran = false;
  const out = await finalizeStall(
    stalledResult,
    baseCtx({
      gate: "make test",
      runGate: async () => {
        ran = true;
        return { command: "make test", exitCode: 0, passed: true, outputTail: "" };
      },
    }),
  );
  assert.equal(ran, false);
  assert.equal(out.gateResult, undefined);
});

test("finalizeStall never raises the incomplete-commit concern", async () => {
  const out = await finalizeStall(
    stalledResult,
    baseCtx({ isWrite: true, headBefore: "aaa", gitDelta: async () => dirtyCommitted }),
  );
  assert.equal(out.concerns, undefined);
});

test("finalizeStall carries stderrTail when the child wrote to stderr", async () => {
  const out = await finalizeStall({ ...stalledResult, stderr: "boom" }, baseCtx());
  assert.equal(out.stderrTail, "boom");
});
