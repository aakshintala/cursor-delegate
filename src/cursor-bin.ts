import { execFileSync } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";

/**
 * Resolve the cursor-agent binary.
 * Order: explicit override -> $CURSOR_AGENT_BIN -> `which cursor-agent` -> ~/.local/bin/cursor-agent.
 */
export function resolveCursorBin(override?: string): string {
  if (override) return override;
  if (process.env.CURSOR_AGENT_BIN) return process.env.CURSOR_AGENT_BIN;
  try {
    const found = execFileSync("which", ["cursor-agent"], {
      encoding: "utf8",
    }).trim();
    if (found) return found;
  } catch {
    // fall through
  }
  return join(homedir(), ".local", "bin", "cursor-agent");
}
