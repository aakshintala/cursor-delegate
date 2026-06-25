import { RUN_STATUSES } from "./types.js";
import type { RunOutput, RunStatus, Usage } from "./types.js";
import type { BackendResult } from "./backends/types.js";

/**
 * Status precedence (#8.3):
 *  1. explicit trailing `STATUS: <X>` line, if X is a known RunStatus;
 *  2. else is_error===false && cleanExit -> DONE;
 *  3. else ERROR.
 */
export function deriveStatus(
  text: string,
  rawIsError: boolean | undefined,
  cleanExit: boolean,
): RunStatus {
  const m = text.match(/STATUS:\s*([A-Z_]+)\s*$/m);
  if (m && (RUN_STATUSES as string[]).includes(m[1])) {
    return m[1] as RunStatus;
  }
  if (rawIsError === false && cleanExit) return "DONE";
  return "ERROR";
}

/** Base RunOutput from the raw result blob (before finalize layers on #3/#6/#7). */
export function toRunOutput(
  res: BackendResult,
  opts: {
    model: string;
    backend: string;
    usage: Usage | null;
    costUsd: number | null;
  },
): RunOutput {
  const text = res.raw.result ?? "";
  return {
    status: deriveStatus(text, res.raw.is_error, res.cleanExit),
    text,
    sessionId: res.raw.session_id ?? null,
    backend: opts.backend,
    model: opts.model,
    usage: opts.usage,
    costUsd: opts.costUsd,
    costEstimated: true,
    durationMs: res.raw.duration_ms ?? null,
  };
}
