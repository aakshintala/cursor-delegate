import { readFile as fsReadFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import type {
  Config,
  HostProfile,
  PriceMap,
  TierMap,
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
  tierMapPath: string;
  priceMapPath: string;
  hostProfilePath?: string;
  readFile?: ReadFileFn;
}

/**
 * Read the bundled defaults + the host profile and merge:
 *   tierMap  = { ...default, ...profile.tierOverrides }
 *   priceMap = { ...default, ...profile.priceOverrides }
 * A missing file (ENOENT) is treated as empty/null, not an error.
 */
export async function loadConfig(opts: LoadConfigOpts): Promise<Config> {
  const readFile = opts.readFile ?? defaultReadFile;
  const profilePath =
    opts.hostProfilePath ??
    process.env.CURSOR_DELEGATE_HOST_PROFILE ??
    defaultHostProfilePath();

  const [defaultTier, defaultPrice, profileRaw] = await Promise.all([
    readJson<TierMap>(readFile, opts.tierMapPath),
    readJson<PriceMap>(readFile, opts.priceMapPath),
    readJson<HostProfile>(readFile, profilePath),
  ]);

  const profile: HostProfile = profileRaw ?? {};
  return {
    tierMap: { ...(defaultTier ?? {}), ...(profile.tierOverrides ?? {}) },
    priceMap: { ...(defaultPrice ?? {}), ...(profile.priceOverrides ?? {}) },
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
