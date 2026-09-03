import { test } from "node:test";
import assert from "node:assert/strict";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { createServer, handleCall, type ServerDeps } from "../src/index.js";
import { buildTools } from "../src/tool-schemas.js";
import type { JobRegistry } from "../src/job-registry.js";
import type { Config } from "../src/types.js";

const config: Config = {
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
  },
  priceMap: {
    "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
  },
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
    lookupAnswer: (id: string) => {
      calls.push(`lookupAnswer:${id}`);
      return { ok: false as const, error: "NOT_FOUND" as const };
    },
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

test("exposes eight tools from buildTools including doctor", () => {
  const tools = buildTools(config);
  assert.equal(tools.length, 8);
  const names = tools.map((t) => t.name);
  assert.deepEqual(names, [
    "cursor_run",
    "cursor_poll",
    "cursor_cancel",
    "cursor_wait",
    "cursor_wait_any",
    "cursor_wait_all",
    "cursor_answer",
    "doctor",
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

test("cursor_run rejects invalid waitMs and background types", async () => {
  const { deps: d } = deps();
  await assert.rejects(
    () => handleCall("cursor_run", { prompt: "x", waitMs: "nope" }, d),
    /waitMs/,
  );
  await assert.rejects(
    () => handleCall("cursor_run", { prompt: "x", background: "false" }, d),
    /background/,
  );
});

test("cursor_run rejects invalid capability and isolation", async () => {
  const { deps: d } = deps();
  await assert.rejects(
    () => handleCall("cursor_run", { prompt: "x", capability: "fly" }, d),
    /capability/,
  );
  await assert.rejects(
    () =>
      handleCall("cursor_run", { prompt: "x", isolation: { type: "Nope" } }, d),
    /isolation/,
  );
});

test("routes each tool to the registry", async () => {
  const { deps: d, calls } = deps();
  await handleCall("cursor_run", { prompt: "x" }, d);
  await handleCall("cursor_poll", { jobId: "a" }, d);
  await handleCall("cursor_cancel", { jobId: "b" }, d);
  await handleCall("cursor_wait", { jobId: "c" }, d);
  await handleCall("cursor_wait_any", { jobIds: ["d"] }, d);
  await handleCall("cursor_wait_all", { jobIds: ["e"] }, d);
  await handleCall("cursor_answer", { jobId: "f", answer: "yes" }, d);
  assert.deepEqual(calls, [
    "dispatch",
    "poll:a",
    "cancel:b",
    "wait:c",
    "waitAny",
    "waitAll",
    "lookupAnswer:f",
  ]);
});

test("an unknown tool throws", async () => {
  const { deps: d } = deps();
  await assert.rejects(() => handleCall("nope", {}, d), /unknown tool/);
});

test("cursor_answer requires jobId and answer", async () => {
  const { deps: d } = deps();
  await assert.rejects(
    () => handleCall("cursor_answer", { answer: "x" }, d),
    /jobId/,
  );
  await assert.rejects(
    () => handleCall("cursor_answer", { jobId: "j1" }, d),
    /answer/,
  );
});

test("cursor_answer returns NOT_FOUND for unknown jobId", async () => {
  const { deps: d, calls } = deps();
  const res = await handleCall(
    "cursor_answer",
    { jobId: "missing", answer: "because v2" },
    d,
  );
  assert.deepEqual(res, { status: "NOT_FOUND" });
  assert.deepEqual(calls, ["lookupAnswer:missing"]);
});

test("doctor rejects non-boolean deep", async () => {
  const { deps: d } = deps();
  await assert.rejects(
    () => handleCall("doctor", { deep: "yes" }, d),
    /deep/,
  );
});

test("doctor returns a DoctorReport shaped object without requiring args", async () => {
  const { deps: d } = deps();
  // Force a missing binary so runDoctor does not exec the real CLI.
  const prev = process.env.CURSOR_AGENT_BIN;
  process.env.CURSOR_AGENT_BIN = "/nonexistent/cursor-agent-for-doctor-test";
  try {
    const report = (await handleCall("doctor", {}, d)) as {
      ok: boolean;
      plugin: { version: string };
      agent: { found: boolean; path: string | null };
      account: { loggedIn: boolean };
      modelMenu: { pricesCheckable: boolean; note: string };
      warnings: string[];
      failures: string[];
    };
    assert.equal(typeof report.plugin.version, "string");
    assert.equal(report.agent.found, false);
    assert.equal(report.ok, false);
    assert.equal(report.modelMenu.pricesCheckable, false);
    assert.match(report.modelMenu.note, /not checkable/i);
    assert.ok(Array.isArray(report.failures));
    assert.ok(report.failures.some((f) => /not found/i.test(f)));
  } finally {
    if (prev === undefined) delete process.env.CURSOR_AGENT_BIN;
    else process.env.CURSOR_AGENT_BIN = prev;
  }
});

test("cursor_answer rejects a job that is not awaiting an answer", async () => {
  const calls: string[] = [];
  const registry = {
    dispatch: async () => ({ status: "RUNNING", jobId: "j1" }),
    poll: () => ({ status: "NOT_FOUND" as const }),
    cancel: async () => ({ status: "NOT_FOUND" as const }),
    wait: async () => ({ status: "NOT_FOUND" as const }),
    waitAny: async () => ({ jobs: {} }),
    waitAll: async () => ({ jobs: {}, allDone: true }),
    killAll: () => {},
    lookupAnswer: (id: string) => {
      calls.push(`lookupAnswer:${id}`);
      return {
        ok: false as const,
        error: "NOT_AWAITING" as const,
        status: "DONE" as const,
      };
    },
  } as unknown as JobRegistry;
  const d: ServerDeps = {
    config,
    registry,
    cliConfig: null,
    serverCwd: "/srv",
  };
  await assert.rejects(
    () =>
      handleCall("cursor_answer", { jobId: "j-done", answer: "x" }, d),
    /job is not awaiting an answer/,
  );
  assert.deepEqual(calls, ["lookupAnswer:j-done"]);
});

test("transport smoke: ListTools + CallTool over a linked in-memory transport", async () => {
  const { deps: d } = deps();
  const server = createServer(d);
  const [clientT, serverT] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: "smoke", version: "0" }, { capabilities: {} });
  await Promise.all([server.connect(serverT), client.connect(clientT)]);
  try {
    const listed = await client.listTools();
    assert.equal(listed.tools.length, 8);
    assert.ok(listed.tools.some((t) => t.name === "cursor_run"));

    const poll = (await client.callTool({
      name: "cursor_poll",
      arguments: { jobId: "missing" },
    })) as { content: Array<{ type: string; text: string }> };
    assert.equal(poll.content[0].type, "text");
    assert.deepEqual(JSON.parse(poll.content[0].text), { status: "NOT_FOUND" });

    await assert.rejects(
      () => client.callTool({ name: "nope", arguments: {} }),
      /unknown tool/,
    );
  } finally {
    await client.close();
    await server.close();
  }
});

test("wait* tools reject non-finite timeoutMs", async () => {
  const { deps: d } = deps();
  for (const name of ["cursor_wait", "cursor_wait_any", "cursor_wait_all"]) {
    const args =
      name === "cursor_wait"
        ? { jobId: "j", timeoutMs: Number.NaN }
        : { jobIds: ["j"], timeoutMs: "5000" };
    await assert.rejects(() => handleCall(name, args, d), /timeoutMs/);
  }
});
