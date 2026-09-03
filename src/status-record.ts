import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import type { PollResult } from "./types.js";

export interface StatusRecordWriter {
  /** Synchronous and MUST NOT throw. Any internal failure is caught and swallowed. */
  write(jobId: string, record: PollResult): void;
}

export function statusRecordPath(jobId: string): string {
  return path.join(os.tmpdir(), "cursor-delegate-jobs", `${jobId}.json`);
}

export function fileStatusRecordWriter(): StatusRecordWriter {
  return {
    write(jobId, record) {
      try {
        const filePath = statusRecordPath(jobId);
        const dir = path.dirname(filePath);
        fs.mkdirSync(dir, { recursive: true });
        const tmpPath = path.join(
          dir,
          `.${jobId}.${process.pid}.${Date.now()}.tmp`,
        );
        fs.writeFileSync(tmpPath, JSON.stringify(record));
        fs.renameSync(tmpPath, filePath);
      } catch {
        // best-effort: never propagate filesystem errors
      }
    },
  };
}
