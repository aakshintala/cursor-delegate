import type { PriceMap, Usage } from "./types.js";

const n = (x: number | undefined): number => (typeof x === "number" ? x : 0);

/**
 * Best-effort USD cost. Always estimated (the CLI emits no cost field).
 * Returns null when usage or a price entry is missing.
 * Alias: bare `gpt-5.5` falls back to `gpt-5.5-medium` for price lookup.
 */
export function computeCost(
  usage: Usage | null | undefined,
  priceMap: PriceMap,
  model: string,
): number | null {
  if (!usage) return null;
  const price =
    priceMap[model] ??
    (model === "gpt-5.5" ? priceMap["gpt-5.5-medium"] : undefined);
  if (!price) return null;
  return (
    (n(usage.inputTokens) * price.input +
      n(usage.outputTokens) * price.output +
      n(usage.cacheReadTokens) * price.cacheRead +
      n(usage.cacheWriteTokens) * price.cacheWrite) /
    1e6
  );
}
