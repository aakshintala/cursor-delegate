import { test } from "node:test";
import assert from "node:assert/strict";
import {
  parseAbout,
  parseModelsList,
  diffConfiguredModels,
  probeAgentVersion,
  probeAccount,
  probeModelMenu,
  checkPluginRegistration,
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

const PLUGIN_ID = "cursor-delegate@cursor-delegate-local";
const SERVER_NAME = "plugin:cursor-delegate:cursor-delegate";
const LEGACY_SERVER_NAME = "cursor-delegate";
const FAKE_HOME = "/fake/home";
const MCP_GET_KEY = `mcp get ${SERVER_NAME}`;
const LEGACY_MCP_GET_KEY = `mcp get ${LEGACY_SERVER_NAME}`;

/** Stub entry: bare legacy name absent (desired post-migration). */
const LEGACY_ABSENT_STUB: Record<string, AgentCommandResult> = {
  [LEGACY_MCP_GET_KEY]: {
    ok: false,
    stdout: "",
    stderr: `No MCP server named "${LEGACY_SERVER_NAME}"`,
    error: "exit 1",
  },
};

function stubPluginAndLegacy(
  pluginScoped: Record<string, AgentCommandResult>,
  legacy: Record<string, AgentCommandResult> = LEGACY_ABSENT_STUB,
): RunAgentCommandFn {
  return stubRun({ ...pluginScoped, ...legacy });
}

function mcpGetStdout(argsPath: string, pluginRoot?: string): string {
  const root =
    pluginRoot ?? `${FAKE_HOME}/work/cursor-delegate`;
  return `plugin:cursor-delegate:cursor-delegate:
  Scope: Dynamic config (from command line)
  Status: ✔ Connected
  Type: stdio
  Command: node
  Args: ${argsPath}
  Environment:
    CLAUDE_PLUGIN_ROOT=${root}
    CLAUDE_PLUGIN_DATA=${FAKE_HOME}/.claude/plugins/data/cursor-delegate-cursor-delegate-local
  Timeout: 600000ms
`;
}

/** Verbatim-style `claude mcp get plugin:cursor-delegate:cursor-delegate` (plugin-sourced). */
function realMcpGetStdout(argsPath: string, pluginRoot?: string): string {
  const root =
    pluginRoot ?? `${FAKE_HOME}/work/cursor-delegate`;
  return `plugin:cursor-delegate:cursor-delegate:
  Scope: Dynamic config (from command line)
  Status: ✔ Connected
  Type: stdio
  Command: node
  Args: ${argsPath}
  Environment:
    CLAUDE_PLUGIN_ROOT=${root}
    CLAUDE_PLUGIN_DATA=${FAKE_HOME}/.claude/plugins/data/cursor-delegate-cursor-delegate-local
  Timeout: 600000ms

To remove this server, run: claude mcp remove plugin:cursor-delegate:cursor-delegate -s user
`;
}

/** Plugin-scoped name reachable but Environment block has no CLAUDE_PLUGIN_ROOT. */
function pluginScopedNotSourcedStdout(argsPath: string): string {
  return `plugin:cursor-delegate:cursor-delegate:
  Scope: Dynamic config (from command line)
  Status: ✔ Connected
  Type: stdio
  Command: node
  Args: ${argsPath}
  Environment:
  Timeout: 600000ms
`;
}

/** Args embeds CLAUDE_PLUGIN_ROOT outside the Environment block (must not count). */
function falsePositiveArgsMcpGetStdout(): string {
  return `plugin:cursor-delegate:cursor-delegate:
  Scope: Dynamic config (from command line)
  Status: ✔ Connected
  Type: stdio
  Command: node
  Args: --env CLAUDE_PLUGIN_ROOT=/tmp/fake dist/index.js
  Environment:
  Timeout: 600000ms
`;
}

/** Plugin-sourced Environment but no Args line (CLI format edge case). */
function noArgsButPluginRootStdout(pluginRoot?: string): string {
  const root =
    pluginRoot ?? `${FAKE_HOME}/work/cursor-delegate`;
  return `plugin:cursor-delegate:cursor-delegate:
  Scope: Dynamic config (from command line)
  Status: ✔ Connected
  Type: stdio
  Command: node
  Environment:
    CLAUDE_PLUGIN_ROOT=${root}
    CLAUDE_PLUGIN_DATA=${FAKE_HOME}/.claude/plugins/data/cursor-delegate-cursor-delegate-local
  Timeout: 600000ms
`;
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

/** Legacy raw `claude mcp add` registration under the bare name (User config). */
function legacyRawMcpGetStdout(argsPath: string): string {
  return `cursor-delegate:
  Scope: User config (available in all your projects)
  Status: ✔ Connected
  Type: stdio
  Command: /Users/amogh.akshintala/.nvm/versions/node/v22.22.3/bin/node
  Args: ${argsPath}
  Environment:
  Timeout: 600000ms
`;
}

test("checkPluginRegistration flags legacy bare name reintroduced", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy(
      {
        [MCP_GET_KEY]: {
          ok: true,
          stdout: mcpGetStdout(
            `${FAKE_HOME}/work/cursor-delegate/dist/index.js`,
          ),
          stderr: "",
        },
      },
      {
        [LEGACY_MCP_GET_KEY]: {
          ok: true,
          stdout: legacyRawMcpGetStdout(
            "/Users/dev/work/cursor-delegate/dist/index.js",
          ),
          stderr: "",
        },
      },
    ),
  });
  assert.equal(r.enabled, true);
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, true);
  assert.equal(r.legacyAbsent, false);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) =>
      /still registered under the bare name "cursor-delegate"/.test(d),
    ),
  );
});

test("checkPluginRegistration ignores CLAUDE_PLUGIN_ROOT outside Environment block", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: falsePositiveArgsMcpGetStdout(),
        stderr: "",
      },
    }),
  });
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, false);
  assert.equal(r.ok, false);
});

test("checkPluginRegistration is plugin-sourced without Args when Environment has CLAUDE_PLUGIN_ROOT", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: noArgsButPluginRootStdout(),
        stderr: "",
      },
    }),
  });
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, true);
  assert.equal(r.legacyAbsent, true);
  assert.equal(r.ok, true);
  assert.deepEqual(r.detail, []);
});

/** Verbatim `claude mcp get plugin:cursor-delegate:cursor-delegate` (plugin-sourced, source repo path). */
const VERBATIM_PLUGIN_SOURCED_MCP_GET = `plugin:cursor-delegate:cursor-delegate:
  Scope: Dynamic config (from command line)
  Status: ✔ Connected
  Type: stdio
  Command: node
  Args: /Users/amogh.akshintala/cursor-delegate//dist/index.js
  Environment:
    CLAUDE_PLUGIN_ROOT=/Users/amogh.akshintala/cursor-delegate/
    CLAUDE_PLUGIN_DATA=/Users/amogh.akshintala/.claude/plugins/data/cursor-delegate-cursor-delegate-local
  Timeout: 600000ms

To remove this server, run: claude mcp remove plugin:cursor-delegate:cursor-delegate ...
`;

test("checkPluginRegistration pins Fact B: verbatim plugin-sourced mcp get with CLAUDE_PLUGIN_ROOT", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      "mcp get plugin:cursor-delegate:cursor-delegate": {
        ok: true,
        stdout: VERBATIM_PLUGIN_SOURCED_MCP_GET,
        stderr: "",
      },
    }),
  });
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, true);
  assert.equal(r.legacyAbsent, true);
  assert.equal(r.ok, true);
  assert.deepEqual(r.detail, []);
});

test("checkPluginRegistration pins Fact A: default serverName queries plugin-scoped mcp name", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      "mcp get plugin:cursor-delegate:cursor-delegate": {
        ok: true,
        stdout: VERBATIM_PLUGIN_SOURCED_MCP_GET,
        stderr: "",
      },
    }),
  });
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, true);
  assert.equal(r.legacyAbsent, true);
});

test("checkPluginRegistration parses verbatim claude mcp get output (regression)", async () => {
  const distPath = `${FAKE_HOME}/work/cursor-delegate/dist/index.js`;
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: (path) => {
      assert.equal(path, `${FAKE_HOME}/.claude/settings.json`);
      return {
        exists: true,
        value: { enabledPlugins: { [PLUGIN_ID]: true } },
      };
    },
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: realMcpGetStdout(distPath),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, true);
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, true);
  assert.equal(r.legacyAbsent, true);
  assert.equal(r.ok, true);
  assert.deepEqual(r.detail, []);
});

test("checkPluginRegistration ok when enabled and mcp get is plugin-sourced", async () => {
  const distPath = `${FAKE_HOME}/work/cursor-delegate/dist/index.js`;
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: (path) => {
      assert.equal(path, `${FAKE_HOME}/.claude/settings.json`);
      return {
        exists: true,
        value: { enabledPlugins: { [PLUGIN_ID]: true } },
      };
    },
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: mcpGetStdout(distPath),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, true);
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, true);
  assert.equal(r.legacyAbsent, true);
  assert.equal(r.ok, true);
  assert.deepEqual(r.detail, []);
});

test("checkPluginRegistration reports disabled when enabledPlugins key is absent", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: {} },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: mcpGetStdout(
          `${FAKE_HOME}/work/cursor-delegate/dist/index.js`,
        ),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, false);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) => /not enabled in settings\.json/.test(d)),
  );
});

test("checkPluginRegistration reports disabled when enabledPlugins value is false", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: false } },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: mcpGetStdout(
          `${FAKE_HOME}/work/cursor-delegate/dist/index.js`,
        ),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, false);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) => /not enabled in settings\.json/.test(d)),
  );
});

test("checkPluginRegistration treats missing settings.json as not enabled", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({ exists: false }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: mcpGetStdout(
          `${FAKE_HOME}/work/cursor-delegate/dist/index.js`,
        ),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, false);
  assert.equal(r.ok, false);
  assert.ok(
    !r.detail.some((d) => /could not be parsed/.test(d)),
    "missing file should not use parse-error detail",
  );
});

test("checkPluginRegistration treats corrupt settings.json as not enabled", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({ exists: true, parseError: true }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: mcpGetStdout(
          `${FAKE_HOME}/work/cursor-delegate/dist/index.js`,
        ),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, false);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) => /could not be parsed/.test(d)),
    "parse error should have distinct detail",
  );
});

test("checkPluginRegistration treats JSON null settings as not enabled", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({ exists: true, value: null }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: mcpGetStdout(
          `${FAKE_HOME}/work/cursor-delegate/dist/index.js`,
        ),
        stderr: "",
      },
    }),
  });
  assert.equal(r.enabled, false);
  assert.equal(r.ok, false);
});

test("checkPluginRegistration reports unreachable when mcp get fails", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: false,
        stdout: "",
        stderr: `No MCP server named "${SERVER_NAME}" found`,
        error: "exit 1",
      },
    }),
  });
  assert.equal(r.reachable, false);
  assert.equal(r.resolvesToPluginInstall, false);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) =>
      new RegExp(`no MCP server named "${SERVER_NAME}"`).test(d),
    ),
  );
});

test("checkPluginRegistration flags reachable plugin-scoped server not plugin-sourced", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: pluginScopedNotSourcedStdout(
          "/Users/dev/work/cursor-delegate/dist/index.js",
        ),
        stderr: "",
      },
    }),
  });
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, false);
  assert.equal(r.legacyAbsent, true);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) => /not plugin-sourced \(no CLAUDE_PLUGIN_ROOT/.test(d)),
  );
});

test("checkPluginRegistration reports not plugin-sourced when output lacks Environment CLAUDE_PLUGIN_ROOT", async () => {
  const r = await checkPluginRegistration({
    homeDir: FAKE_HOME,
    pluginId: PLUGIN_ID,
    serverName: SERVER_NAME,
    readJson: () => ({
      exists: true,
      value: { enabledPlugins: { [PLUGIN_ID]: true } },
    }),
    runCommand: stubPluginAndLegacy({
      [MCP_GET_KEY]: {
        ok: true,
        stdout: "Status: Connected\n",
        stderr: "",
      },
    }),
  });
  assert.equal(r.reachable, true);
  assert.equal(r.resolvesToPluginInstall, false);
  assert.equal(r.ok, false);
  assert.ok(
    r.detail.some((d) => /not plugin-sourced/.test(d)),
    "expected not-plugin-sourced detail",
  );
});

test("runDoctor includes pluginRegistration without affecting ok or failures", async () => {
  const pluginRegistration = {
    enabled: false,
    reachable: false,
    resolvesToPluginInstall: false,
    legacyAbsent: true,
    ok: false,
    detail: ["cursor-delegate@cursor-delegate-local is not enabled in settings.json"],
  };
  const report = await runDoctor({
    config: modelsConfig,
    resolveBin: () => "/fake/cursor-agent",
    binExists: () => true,
    readPackageVersion: async () => "0.1.0",
    checkPluginRegistration: async () => pluginRegistration,
    runCommand: stubRun({
      "--version": { ok: true, stdout: "2026.06.01-abc\n", stderr: "" },
      about: { ok: true, stdout: ABOUT_FIXTURE + "\n", stderr: "" },
      models: { ok: true, stdout: MODELS_FIXTURE + "\n", stderr: "" },
    }),
  });

  assert.deepEqual(report.pluginRegistration, pluginRegistration);
  assert.equal(report.ok, true);
  assert.deepEqual(report.failures, []);
});

test("runDoctor computes pluginRegistration on early return when binary missing", async () => {
  const pluginRegistration = {
    enabled: true,
    reachable: true,
    resolvesToPluginInstall: true,
    legacyAbsent: true,
    ok: true,
    detail: [] as string[],
  };
  const report = await runDoctor({
    config: modelsConfig,
    resolveBin: () => "/missing/cursor-agent",
    binExists: () => false,
    readPackageVersion: async () => "0.1.0",
    checkPluginRegistration: async () => pluginRegistration,
    runCommand: async () => {
      throw new Error("runCommand must not be called when bin is missing");
    },
  });

  assert.equal(report.agent.found, false);
  assert.deepEqual(report.pluginRegistration, pluginRegistration);
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
    runCommand: stubPluginAndLegacy({
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
    runCommand: stubPluginAndLegacy({
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
    runCommand: stubPluginAndLegacy({
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
