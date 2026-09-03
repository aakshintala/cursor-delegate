import { execFile } from "node:child_process";
import type { GateResult } from "./types.js";
import { tail } from "./util.js";

const MAX_BUFFER = 16 * 1024 * 1024;
/**
 * Gates run test suites, so the bound is generous — but it exists: a gate like
 * `sleep infinity` used to hang finalize forever, keeping the job's heartbeat
 * alive and making cancel/shutdown unable to finish the job. Bump via opts if a
 * legitimate gate ever outgrows this.
 */
export const DEFAULT_GATE_TIMEOUT_MS = 10 * 60 * 1000;

export interface GateOpts {
  timeoutMs?: number;
  signal?: AbortSignal;
}

/**
 * Run the postcondition `command` via `/bin/sh -c` in `cwd` (#7).
 * Never throws — always resolves a GateResult. `passed = exitCode === 0`.
 * Bounded: the child is killed (SIGTERM) after `timeoutMs` or on `signal` abort.
 */
export function runGate(
  command: string,
  cwd: string,
  opts: GateOpts = {},
): Promise<GateResult> {
  return new Promise((resolve) => {
    execFile(
      "/bin/sh",
      ["-c", command],
      {
        cwd,
        maxBuffer: MAX_BUFFER,
        timeout: opts.timeoutMs ?? DEFAULT_GATE_TIMEOUT_MS,
        signal: opts.signal,
      },
      (err, stdout, stderr) => {
        const e = err as { killed?: boolean; name?: string } | null;
        const killed = e?.killed === true || e?.name === "AbortError";
        const code = (err as { code?: number | string } | null)?.code;
        const exitCode = typeof code === "number" ? code : err ? 1 : 0;
        const combined = (stdout ?? "") + (stderr ?? "");
        resolve({
          command,
          exitCode,
          passed: exitCode === 0 && !killed,
          outputTail: tail(combined, 2048),
          ...(killed
            ? {
                error: `gate killed after timeout or abort signal (exitCode ${exitCode})`,
              }
            : {}),
        });
      },
    );
  });
}
