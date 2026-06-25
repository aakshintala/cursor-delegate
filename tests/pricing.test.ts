import { test } from "node:test";
import assert from "node:assert/strict";
import { computeCost } from "../src/pricing.js";
import type { PriceMap, Usage } from "../src/types.js";

const priceMap: PriceMap = {
  "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
  "gpt-5.5-medium": { input: 5, output: 30, cacheRead: 0.5, cacheWrite: 0 },
};

const usage: Usage = {
  inputTokens: 1_000_000,
  outputTokens: 1_000_000,
  cacheReadTokens: 0,
  cacheWriteTokens: 0,
};

test("null usage -> null", () => {
  assert.equal(computeCost(null, priceMap, "composer-2.5"), null);
});

test("missing price entry -> null", () => {
  assert.equal(computeCost(usage, priceMap, "unknown-model"), null);
});

test("sums tokens x price / 1e6", () => {
  // 1M input * 0.5 + 1M output * 2.5 = 0.5 + 2.5 = 3.0
  assert.equal(computeCost(usage, priceMap, "composer-2.5"), 3.0);
});

test("bare gpt-5.5 aliases to gpt-5.5-medium", () => {
  // 1M input * 5 + 1M output * 30 = 35
  assert.equal(computeCost(usage, priceMap, "gpt-5.5"), 35);
});

test("missing usage fields are treated as 0", () => {
  const partial = { outputTokens: 1_000_000 } as unknown as Usage;
  assert.equal(computeCost(partial, priceMap, "composer-2.5"), 2.5);
});
