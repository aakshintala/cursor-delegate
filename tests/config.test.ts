import { test } from "node:test";
import assert from "node:assert/strict";
import { loadConfig } from "../src/config.js";

const tierDefault = JSON.stringify({
  "cheap-bulk": { backend: "cursor", model: "composer-2.5" },
  diversity: { backend: "cursor", model: "gpt-5.5-medium" },
});
const priceDefault = JSON.stringify({
  "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
});

function reader(files: Record<string, string>) {
  return async (path: string): Promise<string> => {
    if (path in files) return files[path];
    const err = new Error("ENOENT") as NodeJS.ErrnoException;
    err.code = "ENOENT";
    throw err;
  };
}

test("merges profile overrides over the bundled defaults", async () => {
  const cfg = await loadConfig({
    tierMapPath: "/t.json",
    priceMapPath: "/p.json",
    hostProfilePath: "/profile.json",
    readFile: reader({
      "/t.json": tierDefault,
      "/p.json": priceDefault,
      "/profile.json": JSON.stringify({
        tierOverrides: { "cheap-bulk": { backend: "cursor", model: "override-x" } },
        requiredDeny: ["rm -rf /"],
        deadlineMs: 5000,
      }),
    }),
  });
  assert.equal(cfg.tierMap["cheap-bulk"].model, "override-x");
  assert.equal(cfg.tierMap["diversity"].model, "gpt-5.5-medium");
  assert.deepEqual(cfg.profile.requiredDeny, ["rm -rf /"]);
  assert.equal(cfg.profile.deadlineMs, 5000);
});

test("a missing host profile (ENOENT) yields an empty profile, not an error", async () => {
  const cfg = await loadConfig({
    tierMapPath: "/t.json",
    priceMapPath: "/p.json",
    hostProfilePath: "/does-not-exist.json",
    readFile: reader({ "/t.json": tierDefault, "/p.json": priceDefault }),
  });
  assert.deepEqual(cfg.profile, {});
  assert.equal(cfg.tierMap["diversity"].model, "gpt-5.5-medium");
  assert.ok("composer-2.5" in cfg.priceMap);
});
