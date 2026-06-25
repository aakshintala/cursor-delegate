import { test } from "node:test";
import assert from "node:assert/strict";
import {
  resolveModel,
  DiversityClaudeError,
  TierResolutionError,
} from "../src/tiers.js";
import type { TierMap } from "../src/types.js";

const tierMap: TierMap = {
  "cheap-bulk": { backend: "cursor", model: "composer-2.5" },
  diversity: { backend: "cursor", model: "gpt-5.5-medium" },
};

test("raw model override bypasses tier resolution", () => {
  assert.deepEqual(resolveModel({ model: "foo-1" }, tierMap), {
    backend: "cursor",
    model: "foo-1",
  });
});

test("default tier is cheap-bulk", () => {
  assert.deepEqual(resolveModel({}, tierMap), tierMap["cheap-bulk"]);
});

test("named tier resolves via the map", () => {
  assert.equal(resolveModel({ tier: "diversity" }, tierMap).model, "gpt-5.5-medium");
});

test("unknown tier throws TierResolutionError", () => {
  assert.throws(
    () => resolveModel({ tier: "nope" as never }, tierMap),
    TierResolutionError,
  );
});

test("diversity resolving to a Claude model throws", () => {
  assert.throws(
    () =>
      resolveModel(
        { tier: "diversity" },
        { diversity: { backend: "cursor", model: "claude-sonnet-4" } },
      ),
    DiversityClaudeError,
  );
});

test("diversity with a Claude raw override throws", () => {
  assert.throws(
    () => resolveModel({ tier: "diversity", model: "opus-4" }, tierMap),
    DiversityClaudeError,
  );
});
