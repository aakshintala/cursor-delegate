import { test } from "node:test";
import assert from "node:assert/strict";
import {
  parseAbout,
  parseModelsList,
  diffConfiguredModels,
  probeAgentVersion,
  probeAccount,
  probeModelMenu,
  runDoctor,
  type RunAgentCommandFn,
  type AgentCommandResult,
} from "../src/doctor.js";
import type { Config } from "../src/types.js";

// Real `cursor-agent about` format: whitespace-aligned columns, no colons.
const ABOUT_FIXTURE = `
About Cursor CLI

CLI Version         2026.06.01-abc
Model               Composer 2.5
Subscription Tier   Pro
OS                  darwin (arm64)
User Email          alice@example.com
`.trim();

const MODELS_FIXTURE = `
Available models:
composer-2.5 - Composer 2.5
grok-4.5-xhigh - Grok 4.5
gemini-3.5-flash - Gemini 3.5 Flash
gpt-5.5-high - GPT-5.5 1M High
`.trim();

function stubRun(
  table: Record<string, AgentCommandResult>,
): RunAgentCommandFn {
  return async (_bin, args) => {
    const key = args.join(" ");
    const hit = table[key];
    if (!hit) {
      return {
        ok: false,
        stdout: "",
        stderr: "",
        error: `unexpected args: ${key}`,
      };
    }
    return hit;
  };
}

const modelsConfig: Pick<Config, "models"> = {
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
    "stale-id": {
      label: "Stale",
      family: "other",
      price: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0 },
    },
  },
};

test("parseAbout extracts email, subscription, and current model", () => {
  assert.deepEqual(parseAbout(ABOUT_FIXTURE), {
    email: "alice@example.com",
    subscription: "Pro",
    currentModel: "Composer 2.5",
  });
});

test("parseAbout tolerates Plan / Current Model column labels", () => {
  const out = parseAbout(
    "User Email          bob@corp.io\nPlan                Ultra\nCurrent Model       Grok 4.5 High\n",
  );
  assert.deepEqual(out, {
    email: "bob@corp.io",
    subscription: "Ultra",
    currentModel: "Grok 4.5 High",
  });
});

test("parseAbout returns nulls when fields are missing", () => {
  assert.deepEqual(parseAbout("not logged in\n"), {
    email: null,
    subscription: null,
    currentModel: null,
  });
});

test("parseModelsList extracts leading model ids and skips headers", () => {
  assert.deepEqual(parseModelsList(MODELS_FIXTURE), [
    "composer-2.5",
    "grok-4.5-xhigh",
    "gemini-3.5-flash",
    "gpt-5.5-high",
  ]);
});

test("parseModelsList handles --list-models style bare ids", () => {
  assert.deepEqual(
    parseModelsList("composer-2.5\ngrok-4.5-medium\ngrok-4.5-high\n"),
    ["composer-2.5", "grok-4.5-medium", "grok-4.5-high"],
  );
});

test("parseModelsList skips the trailing Tip prose line", () => {
  // Real `cursor-agent models` ends with a "Tip: use --model <id> ..." line.
  assert.deepEqual(
    parseModelsList(
      "Available models\n\ncomposer-2.5 - Composer 2.5\ngrok-4.5-xhigh - Grok 4.5\n\nTip: use --model <id> (or /model <id> in interactive mode) to switch.\n",
    ),
    ["composer-2.5", "grok-4.5-xhigh"],
  );
});

test("diffConfiguredModels returns configured ids missing from account", () => {
  assert.deepEqual(
    diffConfiguredModels(
      ["composer-2.5", "stale-model", "grok-4.5-xhigh"],
      ["composer-2.5", "grok-4.5-xhigh", "extra-account-only"],
    ),
    ["stale-model"],
  );
});

test("probeAgentVersion returns trimmed --version stdout", async () => {
  const run = stubRun({
    "--version": { ok: true, stdout: "2026.06.01-abc\n", stderr: "" },
  });
  const r = await probeAgentVersion("/fake/cursor-agent", run);
  assert.deepEqual(r, { version: "2026.06.01-abc" });
});

test("probeAgentVersion surfaces command failure", async () => {
  const run = stubRun({
    "--version": {
      ok: false,
      stdout: "",
      stderr: "boom",
      error: "exit 1",
    },
  });
  const r = await probeAgentVersion("/fake/cursor-agent", run);
  assert.equal(r.version, null);
  assert.match(r.error ?? "", /exit 1|boom/);
});

test("probeAccount parses about and marks loggedIn from email", async () => {
  const run = stubRun({
    about: { ok: true, stdout: ABOUT_FIXTURE + "\n", stderr: "" },
  });
  const r = await probeAccount("/fake/cursor-agent", run);
  assert.equal(r.loggedIn, true);
  assert.equal(r.email, "alice@example.com");
  assert.equal(r.subscription, "Pro");
  assert.equal(r.currentModel, "Composer 2.5");
  assert.equal(r.error, undefined);
});

test("probeAccount is not loggedIn when about fails or email missing", async () => {
  const fail = await probeAccount(
    "/fake/cursor-agent",
    stubRun({
      about: { ok: false, stdout: "", stderr: "auth required", error: "exit 1" },
    }),
  );
  assert.equal(fail.loggedIn, false);
  assert.ok(fail.error);

  const noEmail = await probeAccount(
    "/fake/cursor-agent",
    stubRun({
      about: { ok: true, stdout: "Subscription Tier   Hobby\n", stderr: "" },
    }),
  );
  assert.equal(noEmail.loggedIn, false);
  assert.equal(noEmail.email, null);
  assert.equal(noEmail.subscription, "Hobby");
});

test("probeModelMenu uses models output and warns on configured-but-missing", async () => {
  const run = stubRun({
    models: { ok: true, stdout: MODELS_FIXTURE + "\n", stderr: "" },
  });
  const r = await probeModelMenu(
    "/fake/cursor-agent",
    ["composer-2.5", "stale-id", "grok-4.5-xhigh"],
    run,
  );
  assert.deepEqual(r.accountIds, [
    "composer-2.5",
    "grok-4.5-xhigh",
    "gemini-3.5-flash",
    "gpt-5.5-high",
  ]);
  assert.deepEqual(r.missingFromAccount, ["stale-id"]);
  assert.equal(r.pricesCheckable, false);
  assert.match(r.note, /prices? are not checkable/i);
  assert.equal(r.error, undefined);
});

test("probeModelMenu falls back to --list-models when models fails", async () => {
  const run = stubRun({
    models: { ok: false, stdout: "", stderr: "unknown", error: "exit 1" },
    "--list-models": {
      ok: true,
      stdout: "composer-2.5\ngrok-4.5-xhigh\n",
      stderr: "",
    },
  });
  const r = await probeModelMenu(
    "/fake/cursor-agent",
    ["composer-2.5", "missing-one"],
    run,
  );
  assert.deepEqual(r.accountIds, ["composer-2.5", "grok-4.5-xhigh"]);
  assert.deepEqual(r.missingFromAccount, ["missing-one"]);
});

test("probeModelMenu records error when both list commands fail", async () => {
  const run = stubRun({
    models: { ok: false, stdout: "", stderr: "a", error: "exit 1" },
    "--list-models": { ok: false, stdout: "", stderr: "b", error: "exit 2" },
  });
  const r = await probeModelMenu("/fake/cursor-agent", ["composer-2.5"], run);
  assert.equal(r.accountIds, null);
  assert.deepEqual(r.missingFromAccount, []);
  assert.ok(r.error);
});

test("runDoctor happy path: ok with model-menu warnings only", async () => {
  const report = await runDoctor({
    config: modelsConfig,
    resolveBin: () => "/fake/cursor-agent",
    binExists: () => true,
    readPackageVersion: async () => "0.1.0",
    runCommand: stubRun({
      "--version": { ok: true, stdout: "2026.06.01-abc\n", stderr: "" },
      about: { ok: true, stdout: ABOUT_FIXTURE + "\n", stderr: "" },
      models: { ok: true, stdout: MODELS_FIXTURE + "\n", stderr: "" },
    }),
  });

  assert.equal(report.ok, true);
  assert.equal(report.plugin.version, "0.1.0");
  assert.equal(report.agent.found, true);
  assert.equal(report.agent.path, "/fake/cursor-agent");
  assert.equal(report.agent.version, "2026.06.01-abc");
  assert.equal(report.account.loggedIn, true);
  assert.equal(report.account.email, "alice@example.com");
  assert.deepEqual(report.modelMenu.missingFromAccount, ["stale-id"]);
  assert.equal(report.modelMenu.pricesCheckable, false);
  assert.match(report.modelMenu.note, /not checkable/i);
  assert.deepEqual(report.failures, []);
  assert.ok(
    report.warnings.some((w) => /stale-id/.test(w)),
    `expected stale-id warning, got ${JSON.stringify(report.warnings)}`,
  );
});

test("runDoctor fails when binary is missing", async () => {
  const report = await runDoctor({
    config: modelsConfig,
    resolveBin: () => "/missing/cursor-agent",
    binExists: () => false,
    readPackageVersion: async () => "0.1.0",
    runCommand: async () => {
      throw new Error("runCommand must not be called when bin is missing");
    },
  });
  assert.equal(report.ok, false);
  assert.equal(report.agent.found, false);
  assert.ok(report.failures.some((f) => /not found/i.test(f)));
  assert.equal(report.account.loggedIn, false);
  assert.equal(report.modelMenu.accountIds, null);
});

test("runDoctor fails when about has no email (not logged in)", async () => {
  const report = await runDoctor({
    config: { models: { "composer-2.5": modelsConfig.models["composer-2.5"] } },
    resolveBin: () => "/fake/cursor-agent",
    binExists: () => true,
    readPackageVersion: async () => "0.1.0",
    runCommand: stubRun({
      "--version": { ok: true, stdout: "1.0.0\n", stderr: "" },
      about: { ok: true, stdout: "Subscription Tier   Hobby\n", stderr: "" },
      models: {
        ok: true,
        stdout: "composer-2.5 - Composer 2.5\n",
        stderr: "",
      },
    }),
  });
  assert.equal(report.ok, false);
  assert.equal(report.account.loggedIn, false);
  assert.ok(report.failures.some((f) => /not logged in/i.test(f)));
});

test("runDoctor treats model-list command failure as warning not failure", async () => {
  const report = await runDoctor({
    config: { models: { "composer-2.5": modelsConfig.models["composer-2.5"] } },
    resolveBin: () => "/fake/cursor-agent",
    binExists: () => true,
    readPackageVersion: async () => "0.1.0",
    runCommand: stubRun({
      "--version": { ok: true, stdout: "1.0.0\n", stderr: "" },
      about: { ok: true, stdout: ABOUT_FIXTURE + "\n", stderr: "" },
      models: { ok: false, stdout: "", stderr: "nope", error: "exit 1" },
      "--list-models": {
        ok: false,
        stdout: "",
        stderr: "nope2",
        error: "exit 2",
      },
    }),
  });
  assert.equal(report.ok, true);
  assert.equal(report.failures.length, 0);
  assert.ok(report.warnings.some((w) => /model/i.test(w)));
});

test("runDoctor accepts deep and ignores it", async () => {
  const report = await runDoctor({
    config: { models: { "composer-2.5": modelsConfig.models["composer-2.5"] } },
    deep: true,
    resolveBin: () => "/fake/cursor-agent",
    binExists: () => true,
    readPackageVersion: async () => "0.1.0",
    runCommand: stubRun({
      "--version": { ok: true, stdout: "1.0.0\n", stderr: "" },
      about: { ok: true, stdout: ABOUT_FIXTURE + "\n", stderr: "" },
      models: {
        ok: true,
        stdout: "composer-2.5 - Composer 2.5\n",
        stderr: "",
      },
    }),
  });
  assert.equal(report.ok, true);
  assert.deepEqual(report.modelMenu.missingFromAccount, []);
});
