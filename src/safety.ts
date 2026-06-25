export class DenyListError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DenyListError";
  }
}

export interface CliConfig {
  permissions?: { deny?: string[] };
}

/**
 * Fail-closed deny-list check. Before any write capability runs, every pattern in `requiredDeny`
 * must be present in the cursor-agent cli-config `permissions.deny`. An empty requirement passes.
 */
export function verifyDenyList(
  requiredDeny: string[],
  cliConfig: CliConfig | null,
): void {
  if (!requiredDeny || requiredDeny.length === 0) return;
  const deny = new Set(cliConfig?.permissions?.deny ?? []);
  const missing = requiredDeny.filter((p) => !deny.has(p));
  if (missing.length > 0) {
    throw new DenyListError(
      `cursor-agent deny-list is missing required patterns: ${missing.join(", ")}. ` +
        `Add them to ~/.cursor/cli-config.json permissions.deny before running write tools.`,
    );
  }
}
