import { execFile } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveCursorBin } from "./cursor-bin.js";
import type {
  DoctorAccountInfo,
  DoctorModelMenuInfo,
  DoctorReport,
  PluginRegistrationCheck,
  Config,
} from "./types.js";

const MAX_BUFFER = 2 * 1024 * 1024;
const PRICES_NOTE =
  "Prices are not checkable via the CLI (about/models/--list-models return ids/labels only).";

export interface AgentCommandResult {
  ok: boolean;
  stdout: string;
  stderr: string;
  error?: string;
}

export type RunAgentCommandFn = (
  bin: string,
  args: string[],
) => Promise<AgentCommandResult>;

/**
 * Parse `cursor-agent about` output. IMPORTANT: the real CLI prints
 * whitespace-aligned columns (`Field<2+ spaces>Value`), NOT `Label: value`.
 * Observed field labels: "CLI Version", "Model", "Subscription Tier", "User Email".
 * (Verified against cursor-agent 2026.07 — do not switch this back to a colon parser.)
 */
export function parseAbout(
  stdout: string,
): Pick<DoctorAccountInfo, "email" | "subscription" | "currentModel"> {
  const fields = new Map<string, string>();
  for (const line of stdout.split(/\r?\n/)) {
    const m = line.match(/^(\S.*?\S)\s{2,}(\S.*?)\s*$/);
    if (m) fields.set(m[1].toLowerCase(), m[2]);
  }
  const get = (...keys: string[]): string | null => {
    for (const k of keys) {
      const v = fields.get(k);
      if (v && v.length > 0) return v;
    }
    return null;
  };
  return {
    email: get("user email", "email"),
    subscription: get("subscription tier", "subscription", "plan", "tier"),
    currentModel: get("model", "current model"),
  };
}

// Model ids are lowercase (composer-2.5, grok-4.5-xhigh, gpt-5.5-high, ...).
// Anchoring to a lowercase-alnum start skips prose lines — the "Available models"
// header and the trailing "Tip: use --model <id> ..." line — without a per-line
// denylist. (Deliberately case-sensitive: uppercase-leading lines are not ids.)
const MODEL_ID_RE = /^([a-z0-9][a-z0-9._-]*)\b/;

/**
 * Parse `cursor-agent models` / `--list-models` stdout into model ids.
 * Takes the first token on each non-empty, non-header line.
 */
export function parseModelsList(stdout: string): string[] {
  const ids: string[] = [];
  const seen = new Set<string>();
  for (const line of stdout.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    if (/^available models\b/i.test(trimmed)) continue;
    if (/^-+$/.test(trimmed)) continue;
    const m = trimmed.match(MODEL_ID_RE);
    if (!m) continue;
    const id = m[1];
    if (seen.has(id)) continue;
    seen.add(id);
    ids.push(id);
  }
  return ids;
}

/** Configured allow-list ids that are absent from the account model list. */
export function diffConfiguredModels(
  configuredIds: string[],
  accountIds: string[],
): string[] {
  const account = new Set(accountIds);
  return configuredIds.filter((id) => !account.has(id)).sort();
}

/** Default runner: promisified execFile; never throws. */
export function defaultRunAgentCommand(
  bin: string,
  args: string[],
): Promise<AgentCommandResult> {
  return new Promise((resolve) => {
    execFile(
      bin,
      args,
      { encoding: "utf8", maxBuffer: MAX_BUFFER, timeout: 15_000 },
      (err, stdout, stderr) => {
        const out = typeof stdout === "string" ? stdout : "";
        const errOut = typeof stderr === "string" ? stderr : "";
        if (err) {
          const code = (err as NodeJS.ErrnoException & { code?: string | number })
            .code;
          resolve({
            ok: false,
            stdout: out,
            stderr: errOut,
            error:
              err.message ||
              (code !== undefined ? `exit ${code}` : "command failed"),
          });
          return;
        }
        resolve({ ok: true, stdout: out, stderr: errOut });
      },
    );
  });
}

export async function probeAgentVersion(
  bin: string,
  runCommand: RunAgentCommandFn,
): Promise<{ version: string | null; error?: string }> {
  const r = await runCommand(bin, ["--version"]);
  if (!r.ok) {
    return {
      version: null,
      error: r.error ?? (r.stderr.trim() || "cursor-agent --version failed"),
    };
  }
  const version = r.stdout.trim() || null;
  return { version };
}

export async function probeAccount(
  bin: string,
  runCommand: RunAgentCommandFn,
): Promise<DoctorAccountInfo> {
  const r = await runCommand(bin, ["about"]);
  if (!r.ok) {
    return {
      loggedIn: false,
      email: null,
      subscription: null,
      currentModel: null,
      error: r.error ?? (r.stderr.trim() || "cursor-agent about failed"),
    };
  }
  const parsed = parseAbout(r.stdout);
  return {
    loggedIn: parsed.email !== null,
    ...parsed,
  };
}

export async function probeModelMenu(
  bin: string,
  configuredIds: string[],
  runCommand: RunAgentCommandFn,
): Promise<DoctorModelMenuInfo> {
  const sortedConfigured = [...configuredIds].sort();
  const base = {
    configuredIds: sortedConfigured,
    pricesCheckable: false as const,
    note: PRICES_NOTE,
  };

  let list = await runCommand(bin, ["models"]);
  if (!list.ok) {
    list = await runCommand(bin, ["--list-models"]);
  }
  if (!list.ok) {
    return {
      ...base,
      accountIds: null,
      missingFromAccount: [],
      error:
        list.error ??
        (list.stderr.trim() || "cursor-agent models/--list-models failed"),
    };
  }

  const accountIds = parseModelsList(list.stdout);
  return {
    ...base,
    accountIds,
    missingFromAccount: diffConfiguredModels(sortedConfigured, accountIds),
  };
}

export type BinExistsFn = (path: string) => boolean;
export type ReadPackageVersionFn = () => Promise<string>;

export async function defaultReadPackageVersion(): Promise<string> {
  const here = dirname(fileURLToPath(import.meta.url));
  const pkgPath = join(here, "..", "package.json");
  const raw = await readFile(pkgPath, "utf8");
  const pkg = JSON.parse(raw) as { version?: string };
  if (typeof pkg.version !== "string" || pkg.version.length === 0) {
    throw new Error("package.json missing version");
  }
  return pkg.version;
}

/** Parse `claude mcp get` output (indented `Label: value` plus Environment KEY=value lines). */
function parseMcpGet(stdout: string): {
  scope: string | null;
  hasPluginRoot: boolean;
} {
  const fields = new Map<string, string>();
  for (const line of stdout.split(/\r?\n/)) {
    const colonIdx = line.indexOf(":");
    if (colonIdx === -1) continue;
    const label = line.slice(0, colonIdx).trim().toLowerCase();
    if (!label) continue;
    const value = line.slice(colonIdx + 1).trim();
    fields.set(label, value);
  }

  let hasPluginRoot = false;
  const lines = stdout.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const envMatch = lines[i].match(/^(\s*)Environment:\s*$/);
    if (!envMatch) continue;
    const envIndent = envMatch[1].length;
    for (let j = i + 1; j < lines.length; j++) {
      const inner = lines[j];
      if (inner.trim() === "") continue;
      const innerIndent = inner.match(/^(\s*)/)?.[1].length ?? 0;
      if (innerIndent <= envIndent) break;
      if (/^\s*CLAUDE_PLUGIN_ROOT=/.test(inner)) {
        hasPluginRoot = true;
        break;
      }
    }
    break;
  }

  return {
    scope: fields.get("scope") ?? null,
    hasPluginRoot,
  };
}

export type ReadJsonResult =
  | { exists: true; value: unknown }
  | { exists: false }
  | { exists: true; parseError: true };

export interface CheckPluginRegistrationDeps {
  readJson?: (path: string) => ReadJsonResult;
  runCommand?: RunAgentCommandFn;
  homeDir?: string;
  pluginId?: string;
  serverName?: string;
  legacyServerName?: string;
}

function defaultReadJson(path: string): ReadJsonResult {
  if (!existsSync(path)) return { exists: false };
  try {
    const raw = readFileSync(path, "utf8");
    return { exists: true, value: JSON.parse(raw) as unknown };
  } catch {
    return { exists: true, parseError: true };
  }
}

export async function checkPluginRegistration(
  deps?: CheckPluginRegistrationDeps,
): Promise<PluginRegistrationCheck> {
  const readJson = deps?.readJson ?? defaultReadJson;
  const runCommand = deps?.runCommand ?? defaultRunAgentCommand;
  const homeDir = deps?.homeDir ?? homedir();
  const pluginId = deps?.pluginId ?? "cursor-delegate@cursor-delegate-local";
  const serverName = deps?.serverName ?? "plugin:cursor-delegate:cursor-delegate";
  const legacyServerName = deps?.legacyServerName ?? "cursor-delegate";

  const detail: string[] = [];

  const settingsPath = join(homeDir, ".claude", "settings.json");
  const settings = readJson(settingsPath);
  let enabled = false;
  if (settings.exists === false) {
    // not enabled; no detail for missing file
  } else if ("parseError" in settings && settings.parseError) {
    detail.push("settings.json exists but could not be parsed");
  } else if ("value" in settings) {
    const value = settings.value;
    if (
      value !== null &&
      typeof value === "object" &&
      !Array.isArray(value)
    ) {
      const plugins = (
        value as { enabledPlugins?: Record<string, boolean> }
      ).enabledPlugins;
      enabled = plugins?.[pluginId] === true;
    }
    if (!enabled) {
      detail.push(`${pluginId} is not enabled in settings.json`);
    }
  }

  const mcpGet = await runCommand("claude", ["mcp", "get", serverName]);
  let reachable = false;
  let resolvesToPluginInstall = false;
  if (!mcpGet.ok) {
    detail.push(
      `no MCP server named "${serverName}" is currently registered`,
    );
  } else {
    reachable = true;
    const { hasPluginRoot } = parseMcpGet(mcpGet.stdout);
    // Plugin-launched servers have CLAUDE_PLUGIN_ROOT in the Environment block;
    // raw hand-added `claude mcp add` entries never do.
    resolvesToPluginInstall = hasPluginRoot;
    if (!resolvesToPluginInstall) {
      detail.push(
        `${serverName} is registered but not plugin-sourced (no CLAUDE_PLUGIN_ROOT in its environment) — a raw registration is still live`,
      );
    }
  }

  const legacyGet = await runCommand("claude", ["mcp", "get", legacyServerName]);
  const legacyAbsent = !legacyGet.ok;
  if (!legacyAbsent) {
    detail.push(
      `a server is still registered under the bare name "${legacyServerName}" — the legacy raw registration may have been reintroduced`,
    );
  }

  const ok =
    enabled && reachable && resolvesToPluginInstall && legacyAbsent;
  return {
    enabled,
    reachable,
    resolvesToPluginInstall,
    legacyAbsent,
    ok,
    detail,
  };
}

export interface RunDoctorOpts {
  config: Pick<Config, "models">;
  resolveBin?: (override?: string) => string;
  binExists?: BinExistsFn;
  runCommand?: RunAgentCommandFn;
  readPackageVersion?: ReadPackageVersionFn;
  checkPluginRegistration?: () => Promise<PluginRegistrationCheck>;
  /** Reserved; currently ignored. */
  deep?: boolean;
}

export async function runDoctor(opts: RunDoctorOpts): Promise<DoctorReport> {
  void opts.deep; // reserved
  const resolveBin = opts.resolveBin ?? resolveCursorBin;
  const binExists = opts.binExists ?? ((p: string) => existsSync(p));
  const runCommand = opts.runCommand ?? defaultRunAgentCommand;
  const readPackageVersion =
    opts.readPackageVersion ?? defaultReadPackageVersion;
  const checkPluginRegistrationFn =
    opts.checkPluginRegistration ?? (() => checkPluginRegistration());

  const warnings: string[] = [];
  const failures: string[] = [];

  let pluginVersion = "unknown";
  try {
    pluginVersion = await readPackageVersion();
  } catch (e: unknown) {
    failures.push(
      `failed to read plugin version: ${e instanceof Error ? e.message : String(e)}`,
    );
  }

  const path = resolveBin();
  const configuredIds = Object.keys(opts.config.models);
  const pluginRegistration = await checkPluginRegistrationFn();

  if (!binExists(path)) {
    return {
      ok: false,
      plugin: { version: pluginVersion },
      agent: {
        found: false,
        path,
        version: null,
        error: `cursor-agent not found at ${path}`,
      },
      account: {
        loggedIn: false,
        email: null,
        subscription: null,
        currentModel: null,
        error: "skipped: cursor-agent not found",
      },
      modelMenu: {
        configuredIds: [...configuredIds].sort(),
        accountIds: null,
        missingFromAccount: [],
        pricesCheckable: false,
        note: PRICES_NOTE,
        error: "skipped: cursor-agent not found",
      },
      pluginRegistration,
      warnings,
      failures: [
        ...failures,
        `cursor-agent not found at ${path}`,
      ],
    };
  }

  const ver = await probeAgentVersion(path, runCommand);
  if (ver.error) {
    failures.push(`cursor-agent --version failed: ${ver.error}`);
  }

  const account = await probeAccount(path, runCommand);
  if (!account.loggedIn) {
    failures.push(
      account.error
        ? `not logged in to cursor-agent: ${account.error}`
        : "not logged in to cursor-agent (about missing email)",
    );
  }

  const modelMenu = await probeModelMenu(path, configuredIds, runCommand);
  if (modelMenu.error) {
    warnings.push(`model menu check failed: ${modelMenu.error}`);
  }
  for (const id of modelMenu.missingFromAccount) {
    warnings.push(`configured model not on account: ${id}`);
  }

  return {
    ok: failures.length === 0,
    plugin: { version: pluginVersion },
    agent: {
      found: true,
      path,
      version: ver.version,
      ...(ver.error ? { error: ver.error } : {}),
    },
    account,
    modelMenu,
    pluginRegistration,
    warnings,
    failures,
  };
}
