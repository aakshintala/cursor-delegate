import { test } from "node:test";
import assert from "node:assert/strict";
import { formatProgress, progressSinkFrom } from "../src/progress.js";

test("formatProgress with a tool and a tag", () => {
  assert.equal(
    formatProgress({
      lastTool: "shell",
      tokensSoFar: 12,
      elapsedMs: 3400,
      jobTag: "abc123",
    }),
    "abc123 shell · 12 tok · 3s",
  );
});

test("formatProgress falls back to 'thinking' with no tool", () => {
  assert.equal(
    formatProgress({ lastTool: null, tokensSoFar: 0, elapsedMs: 0 }),
    "thinking · 0 tok · 0s",
  );
});

test("progressSinkFrom returns undefined with no progressToken", () => {
  assert.equal(progressSinkFrom({}), undefined);
  assert.equal(progressSinkFrom({ _meta: {} }), undefined);
});

test("sink sends a notification and throttles token-only updates", () => {
  const sent: Array<Record<string, unknown>> = [];
  let now = 0;
  const sink = progressSinkFrom(
    {
      _meta: { progressToken: "tok-1" },
      sendNotification: (n) => {
        sent.push(n.params);
      },
    },
    () => now,
  )!;

  // First emit always goes (tool change from undefined).
  sink({ lastTool: "shell", tokensSoFar: 1, elapsedMs: 0 });
  assert.equal(sent.length, 1);

  // Same tool, <1000ms later -> dropped.
  now = 500;
  sink({ lastTool: "shell", tokensSoFar: 2, elapsedMs: 500 });
  assert.equal(sent.length, 1);

  // Tool change -> emits immediately even within 1000ms.
  now = 600;
  sink({ lastTool: "edit", tokensSoFar: 3, elapsedMs: 600 });
  assert.equal(sent.length, 2);

  // Same tool but >1000ms later -> emits.
  now = 2000;
  sink({ lastTool: "edit", tokensSoFar: 4, elapsedMs: 2000 });
  assert.equal(sent.length, 3);

  // Sequence increments.
  assert.equal(sent[2].progress, 3);
});
