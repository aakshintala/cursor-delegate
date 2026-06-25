import { execFile } from "node:child_process";
import type { GateResult } from "./types.js";
import { tail } from "./util.js";

const MAX_BUFFER = 16 * 1024 * 1024;

/**
 * Run the postcondition `command` via `/bin/sh -c` in `cwd` (#7).
 * Never throws — always resolves a GateResult. `passed = exitCode === 0`.
 */
export function runGate(command: string, cwd: string): Promise<GateResult> {
  return new Promise((resolve) => {
    execFile(
      "/bin/sh",
      ["-c", command],
      { cwd, maxBuffer: MAX_BUFFER },
      (err, stdout, stderr) => {
        const code = (err as { code?: number | string } | null)?.code;
        const exitCode =
          typeof code === "number" ? code : err ? 1 : 0;
        const combined = (stdout ?? "") + (stderr ?? "");
        resolve({
          command,
          exitCode,
          passed: exitCode === 0,
          outputTail: tail(combined, 2048),
        });
      },
    );
  });
}
