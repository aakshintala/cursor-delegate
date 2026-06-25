import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveCursorBin } from "../src/cursor-bin.js";

test("explicit override wins", () => {
  assert.equal(resolveCursorBin("/custom/cursor-agent"), "/custom/cursor-agent");
});

test("$CURSOR_AGENT_BIN is used when no override", () => {
  const prev = process.env.CURSOR_AGENT_BIN;
  process.env.CURSOR_AGENT_BIN = "/env/cursor-agent";
  try {
    assert.equal(resolveCursorBin(), "/env/cursor-agent");
  } finally {
    if (prev === undefined) delete process.env.CURSOR_AGENT_BIN;
    else process.env.CURSOR_AGENT_BIN = prev;
  }
});
