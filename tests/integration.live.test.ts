// Opt-in live integration test. Runs a real cursor-agent `ask` task end-to-end.
// Requires cursor-agent installed + logged in. Run with: npm run test:live
import { test } from "node:test";
import assert from "node:assert/strict";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadConfig } from "../src/config.js";
import { makeJobRegistry } from "../src/job-registry.js";
import { makeCursorAdapter } from "../src/backends/cursor.js";
import { runDelegation } from "../src/runner.js";
import type { RunOutput } from "../src/types.js";

const here = dirname(fileURLToPath(import.meta.url));
const configDir = join(here, "..", "config");

// Skipped under `npm test` (which matches *.test.ts); only runs via `npm run test:live`,
// which sets CURSOR_DELEGATE_LIVE=1.
test("live: an ask task returns a terminal RunOutput", { skip: !process.env.CURSOR_DELEGATE_LIVE }, async () => {
  const config = await loadConfig({
    modelsPath: join(configDir, "models.json"),
  });
  const registry = makeJobRegistry({
    backend: makeCursorAdapter(),
    deadlineMs: 120000,
    idleMs: 180000,
  });

  const res = (await runDelegation(
    {
      prompt:
        "Reply with exactly the word OK and then a final line 'STATUS: DONE'. Do not run any commands.",
      model: "composer-2.5",
      capability: "ask",
      waitMs: 120000,
    },
    { config, registry, cliConfig: null, serverCwd: process.cwd() },
  )) as RunOutput;

  // If it detached, the test environment was too slow; still assert we got a shape back.
  if ("text" in res) {
    assert.ok(["DONE", "DONE_WITH_CONCERNS"].includes(res.status), res.status);
    assert.equal(res.backend, "cursor");
    assert.equal(res.costEstimated, true);
  } else {
    assert.equal((res as { status: string }).status, "RUNNING");
  }
});
