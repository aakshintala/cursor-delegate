import { test } from "node:test";
import assert from "node:assert/strict";
import {
  resolveModel,
  ModelNotAllowedError,
  NonClaudeViolationError,
} from "../src/models.js";
import type { Config } from "../src/types.js";

const base: Pick<Config, "default" | "models"> = {
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
    "grok-4.5-xhigh": {
      label: "Grok 4.5",
      family: "grok",
      price: { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    },
    "claude-sonnet-4": {
      label: "Claude Sonnet 4",
      family: "claude",
      price: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
    },
  },
};

test("omitted model resolves to default", () => {
  const r = resolveModel({}, base);
  assert.equal(r.model, "composer-2.5");
  assert.equal(r.family, "composer");
  assert.deepEqual(r.price, base.models["composer-2.5"].price);
});

test("allowed id resolves with family and price", () => {
  const r = resolveModel({ model: "grok-4.5-xhigh" }, base);
  assert.equal(r.model, "grok-4.5-xhigh");
  assert.equal(r.family, "grok");
  assert.deepEqual(r.price, base.models["grok-4.5-xhigh"].price);
});

test("unknown id throws ModelNotAllowedError", () => {
  assert.throws(
    () => resolveModel({ model: "not-listed" }, base),
    ModelNotAllowedError,
  );
});

test("requireNonClaude rejects an explicit Claude model", () => {
  assert.throws(
    () =>
      resolveModel(
        { model: "claude-sonnet-4", requireNonClaude: true },
        base,
      ),
    NonClaudeViolationError,
  );
});

test("requireNonClaude rejects a Claude default", () => {
  const claudeDefault: Pick<Config, "default" | "models"> = {
    default: "claude-sonnet-4",
    models: base.models,
  };
  assert.throws(
    () => resolveModel({ requireNonClaude: true }, claudeDefault),
    NonClaudeViolationError,
  );
});

test("requireNonClaude passes a non-Claude model", () => {
  const r = resolveModel(
    { model: "grok-4.5-xhigh", requireNonClaude: true },
    base,
  );
  assert.equal(r.model, "grok-4.5-xhigh");
  assert.equal(r.family, "grok");
});

test("requireNonClaude false allows a Claude model", () => {
  const r = resolveModel(
    { model: "claude-sonnet-4", requireNonClaude: false },
    base,
  );
  assert.equal(r.model, "claude-sonnet-4");
  assert.equal(r.family, "claude");
});
