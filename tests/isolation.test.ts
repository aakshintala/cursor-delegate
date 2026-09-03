import { test } from "node:test";
import assert from "node:assert/strict";
import { mapIsolation } from "../src/isolation.js";
import { resolve } from "node:path";

test("None uses the server cwd and locks on it (canonical key)", () => {
  const r = mapIsolation({ type: "None" }, "/srv");
  assert.deepEqual(r.flags, []);
  assert.equal(r.cwd, "/srv");
  assert.equal(r.path, resolve("/srv"));
});

test("CallerProvided canonicalizes equivalent paths to one lock key", () => {
  const r = mapIsolation({ type: "CallerProvided", path: "/repo/." }, "/srv");
  assert.deepEqual(r.flags, ["--workspace", "/repo/."]);
  assert.equal(r.cwd, "/repo/.");
  assert.equal(r.path, resolve("/repo/."));
  assert.equal(r.path, resolve("/repo"));
});

test("CallerProvided maps to --workspace and locks the path", () => {
  const r = mapIsolation({ type: "CallerProvided", path: "/repo" }, "/srv");
  assert.deepEqual(r.flags, ["--workspace", "/repo"]);
  assert.equal(r.cwd, "/repo");
  assert.equal(r.path, "/repo");
});

test("BackendProvided with name + base", () => {
  const r = mapIsolation(
    { type: "BackendProvided", name: "wt1", base: "main" },
    "/srv",
  );
  assert.deepEqual(r.flags, ["--worktree", "wt1", "--worktree-base", "main"]);
  assert.equal(r.cwd, "/srv");
  assert.equal(r.path, null);
});

test("BackendProvided with no name/base is just --worktree", () => {
  const r = mapIsolation({ type: "BackendProvided" }, "/srv");
  assert.deepEqual(r.flags, ["--worktree"]);
});

test("equivalent paths share one lock key across None and CallerProvided", () => {
  const none = mapIsolation({ type: "None" }, "/repo");
  const caller = mapIsolation({ type: "CallerProvided", path: "/repo/." }, "/repo");
  assert.equal(none.path, caller.path);
});
