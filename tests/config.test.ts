import { test } from "node:test";
import assert from "node:assert/strict";
import { loadConfig } from "../src/config.js";

const modelsDefault = JSON.stringify({
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
  },
});

function reader(files: Record<string, string>) {
  return async (path: string): Promise<string> => {
    if (path in files) return files[path];
    const err = new Error("ENOENT") as NodeJS.ErrnoException;
    err.code = "ENOENT";
    throw err;
  };
}

test("loads models.json default and derives priceMap", async () => {
  const cfg = await loadConfig({
    modelsPath: "/models.json",
    hostProfilePath: "/does-not-exist.json",
    readFile: reader({ "/models.json": modelsDefault }),
  });
  assert.equal(cfg.default, "composer-2.5");
  assert.equal(cfg.models["composer-2.5"].family, "composer");
  assert.deepEqual(cfg.priceMap["composer-2.5"], {
    input: 0.5,
    output: 2.5,
    cacheRead: 0.2,
    cacheWrite: 0,
  });
  assert.deepEqual(cfg.profile, {});
});

test("merges host-profile default and models over the bundled map", async () => {
  const cfg = await loadConfig({
    modelsPath: "/models.json",
    hostProfilePath: "/profile.json",
    readFile: reader({
      "/models.json": modelsDefault,
      "/profile.json": JSON.stringify({
        default: "grok-4.5-xhigh",
        models: {
          "gpt-5.5-high": {
            label: "GPT-5.5 1M High",
            family: "gpt",
            price: { input: 5, output: 30, cacheRead: 0.5, cacheWrite: 0 },
          },
        },
        requiredDeny: ["rm -rf /"],
        deadlineMs: 5000,
      }),
    }),
  });
  assert.equal(cfg.default, "grok-4.5-xhigh");
  assert.ok("composer-2.5" in cfg.models);
  assert.ok("gpt-5.5-high" in cfg.models);
  assert.equal(cfg.models["gpt-5.5-high"].family, "gpt");
  assert.deepEqual(cfg.priceMap["gpt-5.5-high"], {
    input: 5,
    output: 30,
    cacheRead: 0.5,
    cacheWrite: 0,
  });
  assert.deepEqual(cfg.profile.requiredDeny, ["rm -rf /"]);
  assert.equal(cfg.profile.deadlineMs, 5000);
});

test("a missing host profile (ENOENT) yields an empty profile, not an error", async () => {
  const cfg = await loadConfig({
    modelsPath: "/models.json",
    hostProfilePath: "/does-not-exist.json",
    readFile: reader({ "/models.json": modelsDefault }),
  });
  assert.deepEqual(cfg.profile, {});
  assert.equal(cfg.default, "composer-2.5");
  assert.ok("composer-2.5" in cfg.priceMap);
});

test("throws when merged default is absent from models", async () => {
  await assert.rejects(
    () =>
      loadConfig({
        modelsPath: "/models.json",
        hostProfilePath: "/profile.json",
        readFile: reader({
          "/models.json": modelsDefault,
          "/profile.json": JSON.stringify({ default: "not-a-real-model" }),
        }),
      }),
    /default/,
  );
});
