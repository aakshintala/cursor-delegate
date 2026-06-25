export interface ProgressUpdate {
  lastTool: string | null;
  tokensSoFar: number;
  elapsedMs: number;
  phase?: string;
  jobTag?: string;
}

export type ProgressSink = (u: ProgressUpdate) => void;

/** "[jobTag ]<lastTool|thinking> · <tok> tok · <sec>s" */
export function formatProgress(u: ProgressUpdate): string {
  const tag = u.jobTag ? `${u.jobTag} ` : "";
  const label = u.lastTool ?? "thinking";
  const sec = Math.round(u.elapsedMs / 1000);
  return `${tag}${label} · ${u.tokensSoFar} tok · ${sec}s`;
}

/** Minimal shape of the MCP request `extra` we depend on. */
export interface McpExtra {
  _meta?: { progressToken?: string | number };
  sendNotification?: (n: {
    method: string;
    params: Record<string, unknown>;
  }) => Promise<void> | void;
}

/**
 * Bridge the registry's ProgressSink to MCP notifications/progress.
 * Returns undefined when the client supplied no progressToken (no progress wanted).
 * Throttle: emit immediately on a lastTool change; otherwise drop updates <1000ms apart.
 */
export function progressSinkFrom(
  extra: McpExtra | undefined,
  now: () => number = Date.now,
): ProgressSink | undefined {
  const token = extra?._meta?.progressToken;
  if (token === undefined || token === null) return undefined;

  let seq = 0;
  let lastEmit = 0;
  let lastTool: string | null | undefined = undefined;

  return (u: ProgressUpdate) => {
    const t = now();
    const toolChanged = u.lastTool !== lastTool;
    if (!toolChanged && t - lastEmit < 1000) return;
    lastTool = u.lastTool;
    lastEmit = t;
    void extra?.sendNotification?.({
      method: "notifications/progress",
      params: {
        progressToken: token,
        progress: ++seq,
        message: formatProgress(u),
      },
    });
  };
}
