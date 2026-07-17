import type { Capability } from "./types.js";

export interface CapabilityResult {
  flags: string[];
  isWrite: boolean;
  /**
   * True when the flags carry `--force` — command execution is non-interactive.
   * Every capability now runs `--force` (read-only `ask`/`plan` need it to run
   * shell commands like `git show` without blocking on an approval prompt), so
   * the runner gates ALL of them on the deny-list, not just write caps.
   * Distinct from `isWrite`, which still gates the write-path lock and change-set.
   */
  forced: boolean;
  downgraded: boolean;
}

/**
 * Map a capability (+ the `allowUnsandboxed` second signal) to cursor-agent flags.
 * `write-unsandboxed` without `allowUnsandboxed:true` is downgraded to sandboxed `write`.
 *
 * All capabilities carry `--force`: headless (`--print`) runs have no interactive
 * approval channel, so without it `--mode ask`/`--mode plan` block forever the moment
 * the agent tries to run a shell command. `--mode plan`/`ask` still bar edits even with
 * `--force`, so read-only + `--force` = "run any read-only command unprompted".
 */
export function mapCapability(
  capability: Capability = "ask",
  allowUnsandboxed = false,
): CapabilityResult {
  switch (capability) {
    case "ask":
      return {
        flags: ["--mode", "ask", "--force"],
        isWrite: false,
        forced: true,
        downgraded: false,
      };
    case "plan":
      return {
        flags: ["--mode", "plan", "--force"],
        isWrite: false,
        forced: true,
        downgraded: false,
      };
    case "write":
      return {
        flags: ["--sandbox", "enabled", "--force"],
        isWrite: true,
        forced: true,
        downgraded: false,
      };
    case "write-unsandboxed":
      if (allowUnsandboxed) {
        return {
          flags: ["--sandbox", "disabled", "--force"],
          isWrite: true,
          forced: true,
          downgraded: false,
        };
      }
      // Missing the second signal -> downgrade to sandboxed write.
      return {
        flags: ["--sandbox", "enabled", "--force"],
        isWrite: true,
        forced: true,
        downgraded: true,
      };
  }
}
