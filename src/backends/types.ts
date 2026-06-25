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
  /** Emits "progress" (ProgressSnapshotRaw) and "stderr" (string). */
  events: EventEmitter;
  done: Promise<BackendResult>;
}

export interface Backend {
  run(spec: JobSpec): BackendHandle;
}
