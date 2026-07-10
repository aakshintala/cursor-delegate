import type { Config, ResolvedModel } from "./types.js";

export class ModelNotAllowedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ModelNotAllowedError";
  }
}

export class NonClaudeViolationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "NonClaudeViolationError";
  }
}

/**
 * Resolve a callable model from the curated allow-list.
 * Order: explicit model -> config.default; then allow-list lookup;
 * then optional requireNonClaude hard reject when family === "claude".
 */
export function resolveModel(
  input: { model?: string; requireNonClaude?: boolean },
  config: Pick<Config, "default" | "models">,
): ResolvedModel {
  const model = input.model ?? config.default;
  const entry = config.models[model];
  if (!entry) {
    throw new ModelNotAllowedError(
      `model "${model}" is not in the allow-list`,
    );
  }
  if (input.requireNonClaude && entry.family === "claude") {
    throw new NonClaudeViolationError(
      `requireNonClaude is set but model "${model}" has family "claude"`,
    );
  }
  return { model, family: entry.family, price: entry.price };
}
