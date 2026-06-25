import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer, handleCall, type ServerDeps } from "../src/index.js";
import { TOOLS } from "../src/tool-schemas.js";
import type { JobRegistry } from "../src/job-registry.js";
import type { Config } from "../src/types.js";

const config: Config = {
  tierMap: { "cheap-bulk": { backend: "cursor", model: "composer-2.5" } },
  priceMap: {},
  profile: {},
};

function fakeRegistry(): { registry: JobRegistry; calls: string[] } {
  const calls: string[] = [];
  const registry = {
    dispatch: async () => {
      calls.push("dispatch");
      return { status: "RUNNING", jobId: "j1" };
    },
    poll: (id: string) => {
      calls.push(`poll:${id}`);
      return { status: "NOT_FOUND" as const };
    },
    cancel: async (id: string) => {
      calls.push(`cancel:${id}`);
      return { status: "NOT_FOUND" as const };
    },
    wait: async (id: string) => {
      calls.push(`wait:${id}`);
      return { status: "NOT_FOUND" as const };
    },
    waitAny: async () => {
      calls.push("waitAny");
      return { jobs: {} };
    },
    waitAll: async () => {
      calls.push("waitAll");
      return { jobs: {}, allDone: true };
    },
    killAll: () => {},
  } as unknown as JobRegistry;
  return { registry, calls };
}

function deps(): { deps: ServerDeps; calls: string[] } {
  const { registry, calls } = fakeRegistry();
  return {
    deps: { config, registry, cliConfig: null, serverCwd: "/srv" },
    calls,
  };
}

test("exposes six tools", () => {
  assert.equal(TOOLS.length, 6);
  const names = TOOLS.map((t) => t.name);
  assert.deepEqual(names, [
    "cursor_run",
    "cursor_poll",
    "cursor_cancel",
    "cursor_wait",
    "cursor_wait_any",
    "cursor_wait_all",
  ]);
});

test("createServer builds without connecting a transport", () => {
  const { deps: d } = deps();
  const server = createServer(d);
  assert.ok(server);
});

test("cursor_run requires a prompt", async () => {
  const { deps: d } = deps();
  await assert.rejects(() => handleCall("cursor_run", {}, d), /prompt/);
});

test("routes each tool to the registry", async () => {
  const { deps: d, calls } = deps();
  await handleCall("cursor_run", { prompt: "x" }, d);
  await handleCall("cursor_poll", { jobId: "a" }, d);
  await handleCall("cursor_cancel", { jobId: "b" }, d);
  await handleCall("cursor_wait", { jobId: "c" }, d);
  await handleCall("cursor_wait_any", { jobIds: ["d"] }, d);
  await handleCall("cursor_wait_all", { jobIds: ["e"] }, d);
  assert.deepEqual(calls, [
    "dispatch",
    "poll:a",
    "cancel:b",
    "wait:c",
    "waitAny",
    "waitAll",
  ]);
});

test("an unknown tool throws", async () => {
  const { deps: d } = deps();
  await assert.rejects(() => handleCall("nope", {}, d), /unknown tool/);
});
