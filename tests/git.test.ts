import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { captureHead, gitDelta, parsePorcelain, resolveWorktreePath } from "../src/git.js";

function makeRepo(): string {
  const dir = mkdtempSync(join(tmpdir(), "cd-git-"));
  const g = (args: string[]) =>
    execFileSync("git", ["-C", dir, ...args], { stdio: "pipe" });
  g(["init", "-q"]);
  g(["config", "user.email", "t@t.com"]);
  g(["config", "user.name", "t"]);
  writeFileSync(join(dir, "a.txt"), "one\n");
  g(["add", "."]);
  g(["commit", "-q", "-m", "init"]);
  return dir;
}

test("parsePorcelain strips the XY prefix and handles renames", () => {
  assert.deepEqual(parsePorcelain(" M src/a.ts"), ["src/a.ts"]);
  assert.deepEqual(parsePorcelain("?? new.txt"), ["new.txt"]);
  assert.deepEqual(parsePorcelain("R  old.txt -> new.txt"), ["new.txt"]);
});

test("parsePorcelain matrix: status codes, quoted paths, renames", () => {
  assert.deepEqual(parsePorcelain('A  added.ts'), ["added.ts"]);
  assert.deepEqual(parsePorcelain(' D deleted.ts'), ["deleted.ts"]);
  assert.deepEqual(parsePorcelain('UU conflicted.ts'), ["conflicted.ts"]);
  assert.deepEqual(parsePorcelain('!! ignored.ts'), ["ignored.ts"]);
  // quoted paths (spaces / non-ASCII) are unquoted
  assert.deepEqual(parsePorcelain('?? "my file.txt"'), ["my file.txt"]);
  assert.deepEqual(parsePorcelain(' M "dir/a b.ts"'), ["dir/a b.ts"]);
  // quoted rename keeps the new path
  assert.deepEqual(parsePorcelain('R  "old name" -> "new name"'), ["new name"]);
  // multi-line output
  assert.deepEqual(parsePorcelain(" M a.ts\n?? b.txt\n"), ["a.ts", "b.txt"]);
});

test("gitDelta returns null outside a repo", async () => {
  const dir = mkdtempSync(join(tmpdir(), "cd-nogit-"));
  try {
    assert.equal(await captureHead(dir), null);
    assert.equal(await gitDelta(dir, null), null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("gitDelta computes commits, files, and dirty state", async () => {
  const dir = makeRepo();
  try {
    const head0 = await captureHead(dir);
    assert.ok(head0);

    const g = (args: string[]) =>
      execFileSync("git", ["-C", dir, ...args], { stdio: "pipe" });
    writeFileSync(join(dir, "a.txt"), "one\ntwo\n");
    g(["commit", "-q", "-am", "change"]);
    // leave an uncommitted new file
    writeFileSync(join(dir, "b.txt"), "untracked\n");

    const cs = (await gitDelta(dir, head0))!;
    assert.ok(cs);
    assert.equal(cs.headBefore, head0);
    assert.notEqual(cs.headAfter, head0);
    assert.equal(cs.newCommits.length, 1);
    assert.deepEqual(cs.filesChanged, ["a.txt"]);
    assert.equal(cs.dirtyAfter, true);
    assert.deepEqual(cs.uncommittedFiles, ["b.txt"]);
    assert.match(cs.diffstat, /a\.txt/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("resolveWorktreePath finds a named worktree", async () => {
  const dir = makeRepo();
  const wtDir = join(dir, "wt-side");
  try {
    execFileSync("git", ["-C", dir, "worktree", "add", "-q", wtDir, "-b", "wt1"], {
      stdio: "pipe",
    });
    assert.ok(await resolveWorktreePath(dir));
    const resolved = await resolveWorktreePath(dir, "wt-side");
    assert.ok(resolved);
    assert.ok(resolved!.endsWith("/wt-side"));
    assert.equal(await resolveWorktreePath(dir, "missing"), null);
    // bare-suffix over-match: a sibling dir ending in the name must not match
    execFileSync(
      "git",
      ["-C", dir, "worktree", "add", "-q", join(dir, "my-wt-side"), "-b", "wt2"],
      { stdio: "pipe" },
    );
    try {
      assert.equal(await resolveWorktreePath(dir, "y-wt-side"), null);
    } finally {
      execFileSync("git", ["-C", dir, "worktree", "remove", "-f", join(dir, "my-wt-side")], {
        stdio: "pipe",
      });
    }
  } finally {
    execFileSync("git", ["-C", dir, "worktree", "remove", "-f", wtDir], {
      stdio: "pipe",
    });
    rmSync(dir, { recursive: true, force: true });
  }
});
