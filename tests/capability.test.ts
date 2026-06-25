import { test } from "node:test";
import assert from "node:assert/strict";
import { mapCapability } from "../src/capability.js";

test("ask is read-only", () => {
  const r = mapCapability("ask");
  assert.deepEqual(r.flags, ["--mode", "ask"]);
  assert.equal(r.isWrite, false);
  assert.equal(r.downgraded, false);
});

test("plan is read-only", () => {
  assert.deepEqual(mapCapability("plan").flags, ["--mode", "plan"]);
  assert.equal(mapCapability("plan").isWrite, false);
});

test("write is sandboxed + forced", () => {
  const r = mapCapability("write");
  assert.deepEqual(r.flags, ["--sandbox", "enabled", "--force"]);
  assert.equal(r.isWrite, true);
  assert.equal(r.downgraded, false);
});

test("write-unsandboxed with allowUnsandboxed disables the sandbox", () => {
  const r = mapCapability("write-unsandboxed", true);
  assert.deepEqual(r.flags, ["--sandbox", "disabled", "--force"]);
  assert.equal(r.isWrite, true);
  assert.equal(r.downgraded, false);
});

test("write-unsandboxed without the second signal is downgraded to write", () => {
  const r = mapCapability("write-unsandboxed", false);
  assert.deepEqual(r.flags, ["--sandbox", "enabled", "--force"]);
  assert.equal(r.isWrite, true);
  assert.equal(r.downgraded, true);
});

test("default capability is ask", () => {
  assert.deepEqual(mapCapability().flags, ["--mode", "ask"]);
});
