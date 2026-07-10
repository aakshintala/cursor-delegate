import { test } from "node:test";
import assert from "node:assert/strict";
import { composePrompt, verifyBlock, statusBlock } from "../src/prompt.js";

test("verifyBlock is null with no commands", () => {
  assert.equal(verifyBlock(), null);
  assert.equal(verifyBlock([]), null);
});

test("verifyBlock lists the allowed commands", () => {
  const b = verifyBlock(["cargo test", "cargo check"])!;
  assert.match(b, /ONLY verification commands/);
  assert.match(b, /`cargo test`, `cargo check`/);
});

test("statusBlock instructs NEEDS_CONTEXT for mid-run questions", () => {
  const b = statusBlock();
  assert.match(b, /STATUS: NEEDS_CONTEXT/);
  assert.match(b, /STATUS: DONE/);
  assert.match(b, /question/i);
});

test("compose always includes the status convention block", () => {
  const out = composePrompt({ prompt: "do the thing" });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 2);
  assert.match(parts[0], /STATUS: NEEDS_CONTEXT/);
  assert.equal(parts[1], "do the thing");
});

test("compose joins preamble + verify + status + prompt", () => {
  const out = composePrompt({
    preamble: "STANDING",
    verifyCommands: ["x test"],
    prompt: "do the thing",
  });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 4);
  assert.equal(parts[0], "STANDING");
  assert.match(parts[1], /ONLY verification/);
  assert.match(parts[2], /STATUS: NEEDS_CONTEXT/);
  assert.equal(parts[3], "do the thing");
});

test("compose omits empty preamble and verify but keeps status block", () => {
  const out = composePrompt({ prompt: "hi" });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 2);
  assert.match(parts[0], /STATUS:/);
  assert.equal(parts[1], "hi");
});

test("compose strips NUL bytes (#1)", () => {
  const out = composePrompt({ prompt: "a\0b\0c" });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts[parts.length - 1], "abc");
  assert.equal(out.includes("\0"), false);
});
