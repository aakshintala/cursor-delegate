import type { Isolation } from "./types.js";

export interface IsolationResult {
  flags: string[];
  cwd: string;
  /** Set only for CallerProvided (the shared working tree the write lock keys on). */
  path: string | null;
}

/** Map an isolation choice to cursor-agent flags + the working directory. */
export function mapIsolation(
  isolation: Isolation,
  serverCwd: string,
): IsolationResult {
  switch (isolation.type) {
    case "None":
      return { flags: [], cwd: serverCwd, path: null };
    case "CallerProvided":
      return {
        flags: ["--workspace", isolation.path],
        cwd: isolation.path,
        path: isolation.path,
      };
    case "BackendProvided": {
      const flags = ["--worktree"];
      if (isolation.name) flags.push(isolation.name);
      if (isolation.base) flags.push("--worktree-base", isolation.base);
      return { flags, cwd: serverCwd, path: null };
    }
  }
}
