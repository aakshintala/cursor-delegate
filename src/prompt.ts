const SEP = "\n\n---\n\n";

/**
 * Render the verify-scope block (#5): the ONLY verification commands the agent may run.
 * Returns null when there are no verify commands.
 */
export function verifyBlock(verifyCommands?: string[]): string | null {
  if (!verifyCommands || verifyCommands.length === 0) return null;
  const list = verifyCommands.map((c) => `\`${c}\``).join(", ");
  return (
    `These are the ONLY verification commands you may run: ${list}. ` +
    "Do not run workspace-wide builds (e.g. `cargo check --workspace`, full test suites) " +
    "or any other build/test command."
  );
}

/**
 * Compose the final prompt: [preamble?, verifyBlock?, prompt] joined by "\n\n---\n\n",
 * then strip all NUL bytes (#1 — spawn throws on a NUL in any argv entry).
 */
export function composePrompt(opts: {
  preamble?: string;
  verifyCommands?: string[];
  prompt: string;
}): string {
  const parts = [
    opts.preamble,
    verifyBlock(opts.verifyCommands),
    opts.prompt,
  ].filter((p): p is string => typeof p === "string" && p.length > 0);
  return parts.join(SEP).replace(/\0/g, "");
}
