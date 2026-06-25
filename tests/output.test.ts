import { test } from "node:test";
import assert from "node:assert/strict";
import { deriveStatus, toRunOutput } from "../src/output.js";
import type { BackendResult } from "../src/backends/types.js";

test("explicit trailing STATUS line wins", () => {
  assert.equal(deriveStatus("work\nSTATUS: BLOCKED", false, true), "BLOCKED");
  assert.equal(deriveStatus("STATUS: NEEDS_CONTEXT\n", false, true), "NEEDS_CONTEXT");
});

test("only the final STATUS line wins, not an earlier one in prose", () => {
  assert.equal(
    deriveStatus("STATUS: NEEDS_CONTEXT\nmore work\nSTATUS: DONE", false, true),
    "DONE",
  );
  assert.equal(
    deriveStatus("mentioned STATUS: NEEDS_CONTEXT in prose\nall good", false, true),
    "DONE",
  );
});

test("unknown STATUS token is ignored, falls through to DONE", () => {
  assert.equal(deriveStatus("STATUS: WHATEVER", false, true), "DONE");
});

test("clean exit + no error -> DONE", () => {
  assert.equal(deriveStatus("done", false, true), "DONE");
});

test("error or non-clean exit -> ERROR", () => {
  assert.equal(deriveStatus("oops", true, true), "ERROR");
  assert.equal(deriveStatus("oops", false, false), "ERROR");
});

test("toRunOutput maps the raw blob", () => {
  const res: BackendResult = {
    raw: {
      result: "hi\nSTATUS: DONE",
      session_id: "s1",
      duration_ms: 1234,
    },
    cleanExit: true,
    stderr: "",
  };
  const out = toRunOutput(res, {
    model: "composer-2.5",
    backend: "cursor",
    usage: null,
    costUsd: null,
  });
  assert.equal(out.status, "DONE");
  assert.equal(out.text, "hi\nSTATUS: DONE");
  assert.equal(out.sessionId, "s1");
  assert.equal(out.durationMs, 1234);
  assert.equal(out.costEstimated, true);
  assert.equal(out.backend, "cursor");
});
