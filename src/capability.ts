import type { Capability } from "./types.js";

export interface CapabilityResult {
  flags: string[];
  isWrite: boolean;
  downgraded: boolean;
}

/**
 * Map a capability (+ the `allowUnsandboxed` second signal) to cursor-agent flags.
 * `write-unsandboxed` without `allowUnsandboxed:true` is downgraded to sandboxed `write`.
 */
export function mapCapability(
  capability: Capability = "ask",
  allowUnsandboxed = false,
): CapabilityResult {
  switch (capability) {
    case "ask":
      return { flags: ["--mode", "ask"], isWrite: false, downgraded: false };
    case "plan":
      return { flags: ["--mode", "plan"], isWrite: false, downgraded: false };
    case "write":
      return {
        flags: ["--sandbox", "enabled", "--force"],
        isWrite: true,
        downgraded: false,
      };
    case "write-unsandboxed":
      if (allowUnsandboxed) {
        return {
          flags: ["--sandbox", "disabled", "--force"],
          isWrite: true,
          downgraded: false,
        };
      }
      // Missing the second signal -> downgrade to sandboxed write.
      return {
        flags: ["--sandbox", "enabled", "--force"],
        isWrite: true,
        downgraded: true,
      };
  }
}
