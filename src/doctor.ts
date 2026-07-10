import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveCursorBin } from "./cursor-bin.js";
import type {
  DoctorAccountInfo,
  DoctorModelMenuInfo,
  DoctorReport,
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

export interface RunDoctorOpts {
  config: Pick<Config, "models">;
  resolveBin?: (override?: string) => string;
  binExists?: BinExistsFn;
  runCommand?: RunAgentCommandFn;
  readPackageVersion?: ReadPackageVersionFn;
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
    warnings,
    failures,
  };
}
