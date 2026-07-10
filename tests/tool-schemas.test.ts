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
  assert.equal(tools.length, 8);
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
      "cursor_answer",
      "doctor",
    ],
  );
});

test("doctor tool schema has optional reserved deep flag and no required fields", () => {
  const tools = buildTools(config);
  const doctor = tools.find((t) => t.name === "doctor");
  assert.ok(doctor);
  assert.match(doctor!.description, /plugin/i);
  assert.match(doctor!.description, /model menu|models\.json/i);
  assert.match(doctor!.description, /warning/i);
  const schema = doctor!.inputSchema as {
    properties: { deep?: { type: string } };
    required?: string[];
  };
  assert.equal(schema.properties.deep?.type, "boolean");
  assert.ok(!schema.required || schema.required.length === 0);
});

test("cursor_answer schema requires jobId and answer", () => {
  const tools = buildTools(config);
  const answer = tools.find((t) => t.name === "cursor_answer");
  assert.ok(answer);
  const schema = answer!.inputSchema as {
    required: string[];
    properties: { jobId: { type: string }; answer: { type: string } };
  };
  assert.deepEqual(schema.required, ["jobId", "answer"]);
  assert.equal(schema.properties.jobId.type, "string");
  assert.equal(schema.properties.answer.type, "string");
  assert.match(answer!.description, /NEEDS_CONTEXT/);
});
