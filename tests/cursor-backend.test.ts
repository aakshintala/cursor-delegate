import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { makeCursorAdapter, type SpawnFn } from "../src/backends/cursor.js";
import { specOf } from "./helpers.js";
import type { ProgressSnapshotRaw } from "../src/backends/types.js";

class FakeChild extends EventEmitter {
  stdout = new EventEmitter();
  stderr = new EventEmitter();
  kill() {}
}

function fakeSpawn(): { spawnFn: SpawnFn; child: FakeChild } {
  const child = new FakeChild();
  const spawnFn: SpawnFn = () => child as never;
  return { spawnFn, child };
}

test("parses NDJSON, emits progress, resolves the result", async () => {
  const { spawnFn, child } = fakeSpawn();
  const backend = makeCursorAdapter({ spawnFn });
  const handle = backend.run(specOf());

  const progress: ProgressSnapshotRaw[] = [];
  handle.events.on("progress", (s: ProgressSnapshotRaw) => progress.push(s));

  // A tool call line (with newline) and then the terminal result WITHOUT a trailing newline.
  child.stdout.emit(
    "data",
    Buffer.from(
      JSON.stringify({
        type: "tool_call",
        subtype: "started",
        tool_call: { shellToolCall: {} },
      }) + "\n",
    ),
  );
  child.stdout.emit(
    "data",
    Buffer.from(
      JSON.stringify({
        type: "result",
        subtype: "success",
        is_error: false,
        result: "all good",
        session_id: "sid",
        usage: { outputTokens: 7 },
      }),
    ),
  );
  child.emit("close", 0);

  const res = await handle.done;
  assert.equal(res.cleanExit, true);
  assert.equal(res.raw.result, "all good");
  assert.equal(res.raw.session_id, "sid");
  assert.ok(progress.length >= 1);
  assert.equal(progress[0].lastTool, "shell");
});

test("a non-zero close code is a non-clean exit; stderr is captured", async () => {
  const { spawnFn, child } = fakeSpawn();
  const backend = makeCursorAdapter({ spawnFn });
  const handle = backend.run(specOf());

  child.stderr.emit("data", Buffer.from("trouble"));
  child.emit("close", 1);

  const res = await handle.done;
  assert.equal(res.cleanExit, false);
  assert.equal(res.stderr, "trouble");
});
