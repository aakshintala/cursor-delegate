import { test } from "node:test";
import assert from "node:assert/strict";
import { initStreamState, parseLine } from "../src/stream.js";

test("ignores blank and non-JSON lines", () => {
  const s = initStreamState();
  assert.equal(parseLine("", s).changed, false);
  assert.equal(parseLine("   ", s).changed, false);
  assert.equal(parseLine("not json", s).changed, false);
});

test("ignores events with no type", () => {
  const s = initStreamState();
  assert.equal(parseLine(JSON.stringify({ foo: 1 }), s).changed, false);
});

test("shell tool_call sets lastTool", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({
      type: "tool_call",
      subtype: "started",
      tool_call: { shellToolCall: {} },
    }),
    s,
  );
  assert.equal(s.lastTool, "shell");
});

test("edit tool_call records the touched path", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({
      type: "tool_call",
      subtype: "started",
      tool_call: { editToolCall: { args: { path: "src/a.ts" } } },
    }),
    s,
  );
  assert.equal(s.lastTool, "edit");
  assert.deepEqual(s.filesTouched, ["src/a.ts"]);
});

test("write/delete tool_calls record path-shaped args generically", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({
      type: "tool_call",
      subtype: "started",
      tool_call: { writeToolCall: { args: { filePath: "src/new.ts" } } },
    }),
    s,
  );
  assert.equal(s.lastTool, "write");
  assert.deepEqual(s.filesTouched, ["src/new.ts"]);

  parseLine(
    JSON.stringify({
      type: "tool_call",
      subtype: "started",
      tool_call: { deleteToolCall: { args: { path: "old.ts" } } },
    }),
    s,
  );
  assert.deepEqual(s.filesTouched, ["src/new.ts", "old.ts"]);
});

test("mcp tool_call uses toolName, falls back to mcp", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({
      type: "tool_call",
      subtype: "started",
      tool_call: { mcpToolCall: { toolName: "search" } },
    }),
    s,
  );
  assert.equal(s.lastTool, "search");
});

test("non-started tool_call does not update lastTool", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({
      type: "tool_call",
      subtype: "completed",
      tool_call: { shellToolCall: {} },
    }),
    s,
  );
  assert.equal(s.lastTool, null);
});

test("assistant text is concatenated and truncated to 200 chars", () => {
  const s = initStreamState();
  const long = "x".repeat(300);
  parseLine(
    JSON.stringify({
      type: "assistant",
      message: { content: [{ type: "text", text: long }] },
    }),
    s,
  );
  assert.equal(s.lastAssistant?.length, 200);
});

test("thinking delta sets the phase", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({ type: "thinking", subtype: "delta", text: "hmm" }),
    s,
  );
  assert.equal(s.phase, "thinking");
});

test("any usage.outputTokens updates tokensSoFar", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({ type: "assistant", usage: { outputTokens: 42 } }),
    s,
  );
  assert.equal(s.tokensSoFar, 42);
});

test("tokensSoFar is monotonic when usage decreases", () => {
  const s = initStreamState();
  parseLine(
    JSON.stringify({ type: "assistant", usage: { outputTokens: 100 } }),
    s,
  );
  parseLine(
    JSON.stringify({ type: "assistant", usage: { outputTokens: 40 } }),
    s,
  );
  assert.equal(s.tokensSoFar, 100);
});

test("result event returns the raw blob", () => {
  const s = initStreamState();
  const r = parseLine(
    JSON.stringify({
      type: "result",
      subtype: "success",
      is_error: false,
      result: "final",
      session_id: "sid",
      duration_ms: 99,
      usage: { outputTokens: 10 },
    }),
    s,
  );
  assert.ok(r.result);
  assert.equal(r.result?.result, "final");
  assert.equal(r.result?.session_id, "sid");
  assert.equal(s.tokensSoFar, 10);
});
