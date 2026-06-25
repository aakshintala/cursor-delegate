import { test } from "node:test";
import assert from "node:assert/strict";
import { buildArgv, runDelegation, type RunnerDeps } from "../src/runner.js";
import { DenyListError } from "../src/safety.js";
import { DiversityClaudeError } from "../src/tiers.js";
import type { JobSpec, Config, DispatchResult } from "../src/types.js";
import type { JobRegistry } from "../src/job-registry.js";

const config: Config = {
  tierMap: {
    "cheap-bulk": { backend: "cursor", model: "composer-2.5" },
    diversity: { backend: "cursor", model: "gpt-5.5-medium" },
  },
  priceMap: {},
  profile: { promptPreamble: "PREAMBLE", requiredDeny: ["rm -rf /"], gate: "make ci" },
};

function fakeRegistry(): { registry: JobRegistry; last: () => JobSpec } {
  let captured: JobSpec | undefined;
  const registry = {
    dispatch: async (spec: JobSpec): Promise<DispatchResult> => {
      captured = spec;
      return { status: "RUNNING", jobId: "j1" };
    },
  } as unknown as JobRegistry;
  return { registry, last: () => captured! };
}

function depsWith(registry: JobRegistry, cliDeny: string[] | null): RunnerDeps {
  return {
    config,
    registry,
    cliConfig: cliDeny === null ? null : { permissions: { deny: cliDeny } },
    serverCwd: "/srv",
    resolveBin: () => "cursor-agent",
    captureHead: async () => "HEAD0",
  };
}

test("buildArgv puts the prompt last and includes the fixed flags", () => {
  const argv = buildArgv({
    model: "m",
    capFlags: ["--mode", "ask"],
    isoFlags: [],
    prompt: "hello",
  });
  assert.deepEqual(argv, [
    "--print",
    "--output-format",
    "stream-json",
    "--trust",
    "--approve-mcps",
    "--model",
    "m",
    "--mode",
    "ask",
    "hello",
  ]);
  assert.equal(argv[argv.length - 1], "hello");
});

test("buildArgv adds --resume when a session is given", () => {
  const argv = buildArgv({
    model: "m",
    capFlags: [],
    isoFlags: [],
    session: "sess-1",
    prompt: "p",
  });
  assert.ok(argv.includes("--resume"));
  assert.equal(argv[argv.indexOf("--resume") + 1], "sess-1");
});

test("an ask run builds a spec with the composed prompt + default gate", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation({ prompt: "do it" }, depsWith(registry, []));
  const spec = last();
  assert.equal(spec.model, "composer-2.5");
  assert.equal(spec.isWrite, false);
  assert.equal(spec.cwd, "/srv");
  assert.equal(spec.gate, "make ci"); // profile default
  assert.equal(spec.headBefore, "HEAD0");
  // prompt is the last argv entry and carries the preamble
  const prompt = spec.argv[spec.argv.length - 1];
  assert.match(prompt, /PREAMBLE/);
  assert.match(prompt, /do it/);
});

test("a write with a satisfied deny-list proceeds", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation(
    { prompt: "edit", capability: "write" },
    depsWith(registry, ["rm -rf /"]),
  );
  assert.equal(last().isWrite, true);
});

test("a write with a missing deny pattern throws before dispatch", async () => {
  const { registry } = fakeRegistry();
  await assert.rejects(
    () => runDelegation({ prompt: "edit", capability: "write" }, depsWith(registry, [])),
    DenyListError,
  );
});

test("diversity + a Claude model throws", async () => {
  const { registry } = fakeRegistry();
  await assert.rejects(
    () =>
      runDelegation(
        { prompt: "x", tier: "diversity", model: "claude-opus" },
        depsWith(registry, []),
      ),
    DiversityClaudeError,
  );
});

test("CallerProvided isolation sets cwd, the lock path, and --workspace", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation(
    {
      prompt: "edit",
      capability: "write",
      isolation: { type: "CallerProvided", path: "/repo" },
    },
    depsWith(registry, ["rm -rf /"]),
  );
  const spec = last();
  assert.equal(spec.cwd, "/repo");
  assert.equal(spec.path, "/repo");
  assert.ok(spec.argv.includes("--workspace"));
});

test("per-call gate and verifyCommands override the profile defaults", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation(
    { prompt: "p", gate: "custom-gate", verifyCommands: ["x test"] },
    depsWith(registry, []),
  );
  const spec = last();
  assert.equal(spec.gate, "custom-gate");
  assert.match(spec.argv[spec.argv.length - 1], /ONLY verification commands/);
});
