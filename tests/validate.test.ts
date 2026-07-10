import { test } from "node:test";
import assert from "node:assert/strict";
import { validateRunInput } from "../src/validate.js";

test("accepts model and requireNonClaude", () => {
  const input = validateRunInput({
    prompt: "do it",
    model: "grok-4.5-xhigh",
    requireNonClaude: true,
  });
  assert.equal(input.prompt, "do it");
  assert.equal(input.model, "grok-4.5-xhigh");
  assert.equal(input.requireNonClaude, true);
});

test("rejects empty model", () => {
  assert.throws(
    () => validateRunInput({ prompt: "x", model: "" }),
    /model/,
  );
});

test("rejects non-boolean requireNonClaude", () => {
  assert.throws(
    () => validateRunInput({ prompt: "x", requireNonClaude: "yes" }),
    /requireNonClaude/,
  );
});

test("tier is no longer recognized (silently ignored)", () => {
  const input = validateRunInput({
    prompt: "x",
    tier: "cheap-bulk",
  });
  assert.equal(input.prompt, "x");
  assert.equal("tier" in input, false);
  assert.equal(input.model, undefined);
});

test("omitted requireNonClaude stays undefined (default false at resolve)", () => {
  const input = validateRunInput({ prompt: "x" });
  assert.equal(input.requireNonClaude, undefined);
});
