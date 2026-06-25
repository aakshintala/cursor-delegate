import { test } from "node:test";
import assert from "node:assert/strict";
import { composePrompt, verifyBlock } from "../src/prompt.js";

test("verifyBlock is null with no commands", () => {
  assert.equal(verifyBlock(), null);
  assert.equal(verifyBlock([]), null);
});

test("verifyBlock lists the allowed commands", () => {
  const b = verifyBlock(["cargo test", "cargo check"])!;
  assert.match(b, /ONLY verification commands/);
  assert.match(b, /`cargo test`, `cargo check`/);
});

test("compose joins preamble + verify + prompt with the separator", () => {
  const out = composePrompt({
    preamble: "STANDING",
    verifyCommands: ["x test"],
    prompt: "do the thing",
  });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 3);
  assert.equal(parts[0], "STANDING");
  assert.match(parts[1], /ONLY verification/);
  assert.equal(parts[2], "do the thing");
});

test("compose omits empty preamble and verify", () => {
  const out = composePrompt({ prompt: "hi" });
  assert.equal(out, "hi");
});

test("compose strips NUL bytes (#1)", () => {
  const out = composePrompt({ prompt: "a\0b\0c" });
  assert.equal(out, "abc");
});
