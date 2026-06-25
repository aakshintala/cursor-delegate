import { test } from "node:test";
import assert from "node:assert/strict";
import { runGate } from "../src/gate.js";

test("a passing command returns passed:true, exit 0", async () => {
  const r = await runGate("echo hello && true", process.cwd());
  assert.equal(r.passed, true);
  assert.equal(r.exitCode, 0);
  assert.match(r.outputTail, /hello/);
});

test("a failing command returns passed:false with a non-zero exit", async () => {
  const r = await runGate("echo boom >&2; exit 3", process.cwd());
  assert.equal(r.passed, false);
  assert.equal(r.exitCode, 3);
  assert.match(r.outputTail, /boom/);
});

test("never throws, even for a nonsense command", async () => {
  const r = await runGate("this-command-does-not-exist-xyz", process.cwd());
  assert.equal(r.passed, false);
  assert.notEqual(r.exitCode, 0);
});
