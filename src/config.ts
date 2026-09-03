import { readFile as fsReadFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type {
  Config,
  HostProfile,
  ModelEntry,
  Price,
  PriceMap,
} from "./types.js";
import type { CliConfig } from "./safety.js";

export type ReadFileFn = (path: string) => Promise<string>;

function defaultReadFile(path: string): Promise<string> {
  return fsReadFile(path, "utf8");
}

async function readJson<T>(
  readFile: ReadFileFn,
  path: string,
): Promise<T | null> {
  try {
    return JSON.parse(await readFile(path)) as T;
  } catch (e: unknown) {
    if ((e as NodeJS.ErrnoException)?.code === "ENOENT") return null;
    throw e;
  }
}

// --- seam validation: config is the one place JSON is trusted; everything
// downstream may assume these shapes (review #5). ---

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function str(obj: Record<string, unknown>, field: string, where: string): string {
  const v = obj[field];
  if (typeof v !== "string") {
    throw new Error(`invalid config: ${where}.${field} must be a string`);
  }
  return v;
}

function optStr(obj: Record<string, unknown>, field: string, where: string): string | undefined {
  const v = obj[field];
  if (v === undefined) return undefined;
  if (typeof v !== "string") {
    throw new Error(`invalid config: ${where}.${field} must be a string`);
  }
  return v;
}

function optStrArray(obj: Record<string, unknown>, field: string, where: string): string[] | undefined {
  const v = obj[field];
  if (v === undefined) return undefined;
  if (
    !Array.isArray(v) ||
    v.some((x) => typeof x !== "string")
  ) {
    throw new Error(`invalid config: ${where}.${field} must be an array of strings`);
  }
  return v as string[];
}

function optFinite(obj: Record<string, unknown>, field: string, where: string): number | undefined {
  const v = obj[field];
  if (v === undefined) return undefined;
  if (typeof v !== "number" || !Number.isFinite(v) || v <= 0) {
    throw new Error(
      `invalid config: ${where}.${field} must be a positive finite number`,
    );
  }
  return v;
}

function optFiniteOrNull(obj: Record<string, unknown>, field: string, where: string): number | null | undefined {
  const v = obj[field];
  if (v === undefined) return undefined;
  if (v === null) return null;
  return optFinite(obj, field, where);
}

function decodePrice(raw: unknown, where: string): Price {
  if (!isRecord(raw)) {
    throw new Error(`invalid config: ${where}.price must be an object`);
  }
  const out: Price = {
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
  };
  for (const k of Object.keys(out) as (keyof Price)[]) {
    const v = raw[k];
    if (typeof v !== "number" || !Number.isFinite(v) || v < 0) {
      throw new Error(
        `invalid config: ${where}.price.${k} must be a non-negative finite number`,
      );
    }
    out[k] = v;
  }
  return out;
}

function decodeModelEntry(raw: unknown, id: string, where: string): ModelEntry {
  if (!isRecord(raw)) {
    throw new Error(`invalid config: ${where}["${id}"] must be an object`);
  }
  return {
    label: str(raw, "label", `${where}["${id}"]`),
    family: str(raw, "family", `${where}["${id}"]`),
    price: decodePrice(raw.price, `${where}["${id}"]`),
  };
}

function decodeModels(raw: unknown, where: string): Record<string, ModelEntry> {
  if (!isRecord(raw)) {
    throw new Error(`invalid config: ${where} must be an object`);
  }
  const out: Record<string, ModelEntry> = {};
  for (const [id, entry] of Object.entries(raw)) {
    out[id] = decodeModelEntry(entry, id, where);
  }
  return out;
}

function decodeHostProfile(raw: unknown): HostProfile {
  if (raw === null || raw === undefined) return {};
  if (!isRecord(raw)) {
    throw new Error("invalid config: host profile must be an object");
  }
  const profile: HostProfile = {};
  const dflt = optStr(raw, "default", "host-profile");
  if (dflt !== undefined) profile.default = dflt;
  const models = raw.models;
  if (models !== undefined) profile.models = decodeModels(models, "host-profile.models");
  const deny = optStrArray(raw, "requiredDeny", "host-profile");
  if (deny !== undefined) profile.requiredDeny = deny;
  const preamble = optStr(raw, "promptPreamble", "host-profile");
  if (preamble !== undefined) profile.promptPreamble = preamble;
  const verify = optStrArray(raw, "verifyCommands", "host-profile");
  if (verify !== undefined) profile.verifyCommands = verify;
  const gate = optStr(raw, "gate", "host-profile");
  if (gate !== undefined) profile.gate = gate;
  const deadline = optFinite(raw, "deadlineMs", "host-profile");
  if (deadline !== undefined) profile.deadlineMs = deadline;
  const idle = optFiniteOrNull(raw, "idleMs", "host-profile");
  if (idle !== undefined) profile.idleMs = idle;
  const toolIdle = optFiniteOrNull(raw, "toolIdleMs", "host-profile");
  if (toolIdle !== undefined) profile.toolIdleMs = toolIdle;
  return profile;
}

export function defaultHostProfilePath(): string {
  return join(homedir(), ".config", "cursor-delegate", "host-profile.json");
}

export function defaultCliConfigPath(): string {
  return join(homedir(), ".cursor", "cli-config.json");
}

export interface LoadConfigOpts {
  modelsPath: string;
  hostProfilePath?: string;
  readFile?: ReadFileFn;
}

function toPriceMap(models: Record<string, ModelEntry>): PriceMap {
  const out: PriceMap = {};
  for (const [id, entry] of Object.entries(models)) {
    out[id] = entry.price;
  }
  return out;
}

/**
 * Read bundled models.json + host profile and merge:
 *   default = profile.default ?? file.default
 *   models  = { ...file.models, ...profile.models }
 * A missing host profile (ENOENT) is treated as empty, not an error.
 */
export async function loadConfig(opts: LoadConfigOpts): Promise<Config> {
  const readFile = opts.readFile ?? defaultReadFile;
  const profilePath =
    opts.hostProfilePath ??
    process.env.CURSOR_DELEGATE_HOST_PROFILE ??
    defaultHostProfilePath();

  const [fileRaw, profileRaw] = await Promise.all([
    readJson<unknown>(readFile, opts.modelsPath),
    readJson<unknown>(readFile, profilePath),
  ]);

  if (!isRecord(fileRaw) || typeof fileRaw.default !== "string") {
    throw new Error(`invalid or missing models file: ${opts.modelsPath}`);
  }
  const fileModels = decodeModels(fileRaw.models, "models.json.models");
  const profile = decodeHostProfile(profileRaw);

  const models: Record<string, ModelEntry> = {
    ...fileModels,
    ...(profile.models ?? {}),
  };
  const defaultModel = profile.default ?? fileRaw.default;

  if (!(defaultModel in models)) {
    throw new Error(
      `default model "${defaultModel}" is not present in the models map`,
    );
  }

  return {
    default: defaultModel,
    models,
    priceMap: toPriceMap(models),
    profile,
  };
}

/** Read the cursor-agent cli-config (for the deny-list). Missing file -> null. */
export async function loadCliConfig(
  path: string,
  readFile: ReadFileFn = defaultReadFile,
): Promise<CliConfig | null> {
  const raw = await readJson<unknown>(readFile, path);
  if (raw === null) return null;
  if (!isRecord(raw)) {
    throw new Error(`invalid config: ${path} must be an object`);
  }
  const perms = raw.permissions;
  if (perms !== undefined) {
    if (!isRecord(perms)) {
      throw new Error(`invalid config: ${path}.permissions must be an object`);
    }
    const deny = perms.deny;
    if (
      deny !== undefined &&
      (!Array.isArray(deny) || deny.some((x) => typeof x !== "string"))
    ) {
      throw new Error(
        `invalid config: ${path}.permissions.deny must be an array of strings`,
      );
    }
  }
  return raw as CliConfig;
}
