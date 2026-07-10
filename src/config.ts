import { readFile as fsReadFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type {
  Config,
  HostProfile,
  ModelEntry,
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

interface ModelsFile {
  default: string;
  models: Record<string, ModelEntry>;
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
    readJson<ModelsFile>(readFile, opts.modelsPath),
    readJson<HostProfile>(readFile, profilePath),
  ]);

  if (!fileRaw || !fileRaw.models || typeof fileRaw.default !== "string") {
    throw new Error(`invalid or missing models file: ${opts.modelsPath}`);
  }

  const profile: HostProfile = profileRaw ?? {};
  const models: Record<string, ModelEntry> = {
    ...fileRaw.models,
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
  return readJson<CliConfig>(readFile, path);
}
