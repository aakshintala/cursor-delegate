import { resolve } from "node:path";
import type { Isolation } from "./types.js";

export interface IsolationResult {
  flags: string[];
  cwd: string;
  /** Set only for CallerProvided (the shared working tree the write lock keys on). */
  path: string | null;
}

/**
 * Map an isolation choice to cursor-agent flags + the working directory.
 * `path` is the canonical lock identity for shared-write serialization (#4):
 * CallerProvided paths are resolved (`/repo` and `/repo/.` lock the same key),
 * so the caller keys the write lock on this value, never the raw input path.
 */
export function mapIsolation(
  isolation: Isolation,
  serverCwd: string,
): IsolationResult {
  switch (isolation.type) {
    case "None":
      return { flags: [], cwd: serverCwd, path: resolve(serverCwd) };
    case "CallerProvided":
      return {
        flags: ["--workspace", isolation.path],
        cwd: isolation.path,
        path: resolve(isolation.path),
      };
    case "BackendProvided": {
      const flags = ["--worktree"];
      if (isolation.name) flags.push(isolation.name);
      if (isolation.base) flags.push("--worktree-base", isolation.base);
      return { flags, cwd: serverCwd, path: null };
    }
  }
}
