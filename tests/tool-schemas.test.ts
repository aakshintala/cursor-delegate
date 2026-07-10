import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildRecommendedModelsBlurb,
  buildRunInputSchema,
  buildTools,
} from "../src/tool-schemas.js";
import type { ModelEntry } from "../src/types.js";

const models: Record<string, ModelEntry> = {
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
};

const config = { default: "composer-2.5", models };

test("blurb lists id, label, and in/out prices", () => {
  const blurb = buildRecommendedModelsBlurb(models);
  assert.match(blurb, /composer-2\.5 — Composer 2\.5 — \$0\.5\/\$2\.5/);
  assert.match(blurb, /grok-4\.5-xhigh — Grok 4\.5 — \$2\/\$6/);
});

test("run input schema enum is the allow-list ids", () => {
  const schema = buildRunInputSchema(config) as {
    properties: {
      model: { enum: string[] };
      tier?: unknown;
      requireNonClaude: { type: string };
    };
  };
  assert.deepEqual(
    [...schema.properties.model.enum].sort(),
    ["composer-2.5", "grok-4.5-xhigh"],
  );
  assert.equal(schema.properties.tier, undefined);
  assert.equal(schema.properties.requireNonClaude.type, "boolean");
});

test("buildTools wires cursor_run description with blurb and default", () => {
  const tools = buildTools(config);
  assert.equal(tools.length, 6);
  const run = tools.find((t) => t.name === "cursor_run");
  assert.ok(run);
  assert.match(run!.description, /composer-2\.5 — Composer 2\.5/);
  assert.match(run!.description, /Default model: composer-2\.5/);
  assert.equal(
    (run!.inputSchema as { properties: { model: { enum: string[] } } })
      .properties.model.enum.includes("grok-4.5-xhigh"),
    true,
  );
  assert.deepEqual(
    tools.map((t) => t.name),
    [
      "cursor_run",
      "cursor_poll",
      "cursor_cancel",
      "cursor_wait",
      "cursor_wait_any",
      "cursor_wait_all",
    ],
  );
});
