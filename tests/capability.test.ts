import { test } from "node:test";
import assert from "node:assert/strict";
import { mapCapability } from "../src/capability.js";

test("ask carries --force so read-only shell runs non-interactively", () => {
  const r = mapCapability("ask");
  assert.deepEqual(r.flags, ["--mode", "ask", "--force"]);
  assert.equal(r.isWrite, false);
  assert.equal(r.forced, true);
  assert.equal(r.downgraded, false);
});

test("plan carries --force so read-only shell runs non-interactively", () => {
  const r = mapCapability("plan");
  assert.deepEqual(r.flags, ["--mode", "plan", "--force"]);
  assert.equal(r.isWrite, false);
  assert.equal(r.forced, true);
  assert.equal(r.downgraded, false);
});

test("write is sandboxed + forced", () => {
  const r = mapCapability("write");
  assert.deepEqual(r.flags, ["--sandbox", "enabled", "--force"]);
  assert.equal(r.isWrite, true);
  assert.equal(r.forced, true);
  assert.equal(r.downgraded, false);
});

test("write-unsandboxed with allowUnsandboxed disables the sandbox", () => {
  const r = mapCapability("write-unsandboxed", true);
  assert.deepEqual(r.flags, ["--sandbox", "disabled", "--force"]);
  assert.equal(r.isWrite, true);
  assert.equal(r.forced, true);
  assert.equal(r.downgraded, false);
});

test("write-unsandboxed without the second signal is downgraded to write", () => {
  const r = mapCapability("write-unsandboxed", false);
  assert.deepEqual(r.flags, ["--sandbox", "enabled", "--force"]);
  assert.equal(r.isWrite, true);
  assert.equal(r.forced, true);
  assert.equal(r.downgraded, true);
});

test("default capability is ask", () => {
  const r = mapCapability();
  assert.deepEqual(r.flags, ["--mode", "ask", "--force"]);
  assert.equal(r.forced, true);
});
