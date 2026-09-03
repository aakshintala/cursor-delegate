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

test("never throws for an unresolvable cwd", async () => {
  const r = await runGate("true", "/no/such/dir-xyz");
  assert.equal(r.passed, false);
  assert.notEqual(r.exitCode, 0);
});

test("outputTail is capped at the last 2048 bytes", async () => {
  const r = await runGate("seq 1 2000", process.cwd());
  assert.equal(r.passed, true);
  assert.equal(r.command, "seq 1 2000");
  assert.ok(Buffer.byteLength(r.outputTail, "utf8") <= 2048);
  assert.match(r.outputTail, /2000/);
});
