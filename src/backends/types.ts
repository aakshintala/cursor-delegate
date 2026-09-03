import type { ChildProcess } from "node:child_process";
import type { EventEmitter } from "node:events";
import type { JobSpec, RawCursorJson } from "../types.js";

/** What the backend resolves when the child closes. */
export interface BackendResult {
  raw: RawCursorJson;
  cleanExit: boolean;
  stderr: string;
}

/** Snapshot of live progress fields emitted on each "progress" event. */
export interface ProgressSnapshotRaw {
  lastTool: string | null;
  tokensSoFar: number;
  lastAssistant: string | null;
  filesTouched: string[];
  phase: string | null;
}

export interface BackendHandle {
  child: ChildProcess;
  /**
   * Emits "progress" (ProgressSnapshotRaw), "stderr" (string), and "activity" (no payload) —
   * "activity" fires on any raw stdout chunk, even one that doesn't complete a parseable line, so
   * the idle watchdog can treat it as liveness without waiting for a full semantic event.
   */
  events: EventEmitter;
  done: Promise<BackendResult>;
}

export interface Backend {
  run(spec: JobSpec): BackendHandle;
}
