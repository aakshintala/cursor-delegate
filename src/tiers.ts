import type { ResolvedModel, Tier, TierMap } from "./types.js";

export class DiversityClaudeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DiversityClaudeError";
  }
}

export class TierResolutionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TierResolutionError";
  }
}

const CLAUDE_RE = /claude|opus|sonnet|haiku/i;

/**
 * Resolve a model. Order: raw `model` override -> named `tier` -> default `cheap-bulk`.
 * Enforces the diversity-non-Claude contract on both the override and the resolved model.
 */
export function resolveModel(
  input: { tier?: Tier; model?: string },
  tierMap: TierMap,
): ResolvedModel {
  if (input.model) {
    if (input.tier === "diversity" && CLAUDE_RE.test(input.model)) {
      throw new DiversityClaudeError(
        `diversity tier must be non-Claude, got "${input.model}"`,
      );
    }
    return { backend: "cursor", model: input.model };
  }

  const tier = input.tier ?? "cheap-bulk";
  const resolved = tierMap[tier];
  if (!resolved) {
    throw new TierResolutionError(`unknown tier "${tier}"`);
  }
  if (tier === "diversity" && CLAUDE_RE.test(resolved.model)) {
    throw new DiversityClaudeError(
      `diversity tier resolves to a Claude model "${resolved.model}"`,
    );
  }
  return resolved;
}
