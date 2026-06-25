import { execFile } from "node:child_process";
import type { ChangeSet } from "./types.js";

const MAX_BUFFER = 16 * 1024 * 1024;

/** Run `git -C <cwd> <args>`. Resolves raw stdout, or null on any failure (never throws). */
function git(cwd: string, args: string[]): Promise<string | null> {
  return new Promise((resolve) => {
    execFile(
      "git",
      ["-C", cwd, ...args],
      { maxBuffer: MAX_BUFFER },
      (err, stdout) => {
        if (err) resolve(null);
        else resolve(stdout.toString());
      },
    );
  });
}

/** Resolve a ref (default HEAD) to a commit SHA in a working tree (null if not a repo / unknown ref). */
export async function captureHead(
  cwd: string,
  ref = "HEAD",
): Promise<string | null> {
  const out = await git(cwd, ["rev-parse", ref]);
  return out === null ? null : out.trim() || null;
}

function unquote(p: string): string {
  if (p.startsWith('"') && p.endsWith('"') && p.length >= 2) {
    try {
      return JSON.parse(p) as string;
    } catch {
      return p.slice(1, -1);
    }
  }
  return p;
}

/** Parse `git status --porcelain` output into a list of paths. */
export function parsePorcelain(out: string): string[] {
  return out
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => {
      // Strip the 2-char XY status + 1 space prefix.
      let p = line.slice(3);
      const arrow = p.indexOf(" -> ");
      if (arrow !== -1) p = p.slice(arrow + 4); // rename: keep the new path
      return unquote(p);
    });
}

/**
 * Tool-computed git change-set (#6). Best-effort: any git failure yields null, never throws.
 * Not a repo (rev-parse HEAD fails) -> null.
 */
/** Resolve a git worktree path under `repoCwd` (null if not found or not a repo). */
export async function resolveWorktreePath(
  repoCwd: string,
  name?: string,
): Promise<string | null> {
  const out = await git(repoCwd, ["worktree", "list", "--porcelain"]);
  if (!out) return null;

  const worktrees: string[] = [];
  for (const line of out.split("\n")) {
    if (line.startsWith("worktree ")) {
      worktrees.push(line.slice("worktree ".length));
    }
  }
  if (worktrees.length === 0) return null;

  const primary = worktrees[0]!;
  const secondary = worktrees.filter((wt) => wt !== primary);

  if (name) {
    const match = worktrees.find(
      (wt) =>
        wt === name ||
        wt.endsWith(`/${name}`) ||
        wt.endsWith(name),
    );
    return match ?? null;
  }

  if (secondary.length === 1) return secondary[0]!;
  return null;
}

export async function gitDelta(
  cwd: string,
  headBefore: string | null,
): Promise<ChangeSet | null> {
  const headAfter = await captureHead(cwd);
  if (headAfter === null) return null; // not a repo

  let newCommits: string[] = [];
  let filesChanged: string[] = [];
  let diffstat = "";

  if (headBefore) {
    const [rl, fc, ds] = await Promise.all([
      git(cwd, ["rev-list", `${headBefore}..HEAD`]),
      git(cwd, ["diff", "--name-only", headBefore]),
      git(cwd, ["diff", "--stat", headBefore]),
    ]);
    newCommits = rl ? rl.split("\n").map((s) => s.trim()).filter(Boolean) : [];
    filesChanged = fc ? fc.split("\n").map((s) => s.trim()).filter(Boolean) : [];
    diffstat = ds ? ds.trimEnd() : "";
  }

  const porcelain = await git(cwd, ["status", "--porcelain"]);
  const uncommittedFiles = porcelain ? parsePorcelain(porcelain) : [];

  return {
    headBefore,
    headAfter,
    newCommits,
    filesChanged,
    diffstat,
    uncommittedFiles,
    dirtyAfter: uncommittedFiles.length > 0,
  };
}
