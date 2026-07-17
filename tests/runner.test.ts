import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildArgv,
  runDelegation,
  answerDelegation,
  type RunnerDeps,
} from "../src/runner.js";
import { DenyListError } from "../src/safety.js";
import { NonClaudeViolationError } from "../src/models.js";
import type { AnswerLookup } from "../src/job-registry.js";
import type { JobRegistry } from "../src/job-registry.js";
import type {
  JobSpec,
  Config,
  DispatchResult,
  ResumeContext,
} from "../src/types.js";

const config: Config = {
  default: "composer-2.5",
  models: {
    "composer-2.5": {
      label: "Composer 2.5",
      family: "composer",
      price: { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    },
    "grok-4.5-xhigh": {
      label: "Grok 4.5",
      family: "grok",
      price: { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    },
    "claude-sonnet-4": {
      label: "Claude Sonnet 4",
      family: "claude",
      price: { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
    },
  },
  priceMap: {
    "composer-2.5": { input: 0.5, output: 2.5, cacheRead: 0.2, cacheWrite: 0 },
    "grok-4.5-xhigh": { input: 2, output: 6, cacheRead: 0.5, cacheWrite: 0 },
    "claude-sonnet-4": { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 0 },
  },
  profile: {
    promptPreamble: "PREAMBLE",
    requiredDeny: ["rm -rf /"],
    gate: "make ci",
  },
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
    "--",
    "hello",
  ]);
  assert.equal(argv[argv.length - 1], "hello");
});

test("buildArgv inserts -- before the prompt so dash-prefixed prompts are positional", () => {
  const argv = buildArgv({
    model: "m",
    capFlags: [],
    isoFlags: [],
    prompt: "--help",
  });
  const sep = argv.indexOf("--");
  assert.ok(sep >= 0);
  assert.equal(argv[sep + 1], "--help");
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
  await runDelegation({ prompt: "do it" }, depsWith(registry, ["rm -rf /"]));
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

test("a read-only ask with a missing deny pattern throws before dispatch", async () => {
  // ask/plan carry --force now, so they run shell non-interactively and must be
  // gated on the deny-list exactly like write caps.
  const { registry } = fakeRegistry();
  await assert.rejects(
    () => runDelegation({ prompt: "look", capability: "ask" }, depsWith(registry, [])),
    DenyListError,
  );
});

test("a read-only plan with a missing deny pattern throws before dispatch", async () => {
  const { registry } = fakeRegistry();
  await assert.rejects(
    () => runDelegation({ prompt: "plan it", capability: "plan" }, depsWith(registry, [])),
    DenyListError,
  );
});

test("requireNonClaude + a Claude model throws", async () => {
  const { registry } = fakeRegistry();
  await assert.rejects(
    () =>
      runDelegation(
        {
          prompt: "x",
          model: "claude-sonnet-4",
          requireNonClaude: true,
        },
        depsWith(registry, []),
      ),
    NonClaudeViolationError,
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

test("BackendProvided captures the worktree base commit against serverCwd", async () => {
  const { registry, last } = fakeRegistry();
  const calls: Array<{ cwd: string; ref?: string }> = [];
  const deps = depsWith(registry, ["rm -rf /"]);
  deps.captureHead = async (cwd, ref) => {
    calls.push({ cwd, ref });
    return "BASE0";
  };
  await runDelegation(
    {
      prompt: "edit",
      capability: "write",
      isolation: { type: "BackendProvided", name: "wt-1", base: "main" },
    },
    deps,
  );
  const spec = last();
  // HEAD is read from the server repo at the worktree's base ref, not the (absent) worktree.
  assert.deepEqual(calls, [{ cwd: "/srv", ref: "main" }]);
  assert.equal(spec.headBefore, "BASE0");
  assert.equal(spec.worktreeName, "wt-1");
});

test("BackendProvided with no base defaults to server HEAD", async () => {
  const { registry } = fakeRegistry();
  const calls: Array<{ cwd: string; ref?: string }> = [];
  const deps = depsWith(registry, ["rm -rf /"]);
  deps.captureHead = async (cwd, ref) => {
    calls.push({ cwd, ref });
    return "HEAD0";
  };
  await runDelegation(
    { prompt: "edit", capability: "write", isolation: { type: "BackendProvided" } },
    deps,
  );
  // ref undefined -> captureHead falls back to HEAD.
  assert.deepEqual(calls, [{ cwd: "/srv", ref: undefined }]);
});

test("per-call gate and verifyCommands override the profile defaults", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation(
    { prompt: "p", gate: "custom-gate", verifyCommands: ["x test"] },
    depsWith(registry, ["rm -rf /"]),
  );
  const spec = last();
  assert.equal(spec.gate, "custom-gate");
  assert.match(spec.argv[spec.argv.length - 1], /ONLY verification commands/);
});

test("JobSpec.resumeContext captures isolation, capability, verifyCommands, gate, model", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation(
    {
      prompt: "plan it",
      model: "grok-4.5-xhigh",
      requireNonClaude: true,
      capability: "plan",
      isolation: { type: "CallerProvided", path: "/repo" },
      verifyCommands: ["x test"],
      gate: "custom-gate",
      allowPartialCommit: true,
    },
    depsWith(registry, ["rm -rf /"]),
  );
  const spec = last();
  assert.deepEqual(spec.resumeContext, {
    model: "grok-4.5-xhigh",
    requireNonClaude: true,
    capability: "plan",
    allowUnsandboxed: false,
    isolation: { type: "CallerProvided", path: "/repo" },
    verifyCommands: ["x test"],
    gate: "custom-gate",
    allowPartialCommit: true,
  });
});

test("JobSpec.resumeContext defaults capability ask and profile gate", async () => {
  const { registry, last } = fakeRegistry();
  await runDelegation({ prompt: "do it" }, depsWith(registry, ["rm -rf /"]));
  const ctx = last().resumeContext;
  assert.equal(ctx.model, "composer-2.5");
  assert.equal(ctx.capability, "ask");
  assert.equal(ctx.allowUnsandboxed, false);
  assert.deepEqual(ctx.isolation, { type: "None" });
  assert.equal(ctx.gate, "make ci");
  assert.equal(ctx.allowPartialCommit, false);
  assert.equal(ctx.verifyCommands, undefined);
  assert.equal(ctx.requireNonClaude, undefined);
});

function registryWithLookup(
  lookup: (jobId: string) => AnswerLookup,
): { registry: JobRegistry; last: () => JobSpec } {
  let captured: JobSpec | undefined;
  const registry = {
    dispatch: async (spec: JobSpec): Promise<DispatchResult> => {
      captured = spec;
      return { status: "RUNNING", jobId: "j-resume" };
    },
    lookupAnswer: lookup,
  } as unknown as JobRegistry;
  return { registry, last: () => captured! };
}

const parkedCtx: ResumeContext = {
  model: "grok-4.5-xhigh",
  requireNonClaude: true,
  capability: "write",
  allowUnsandboxed: false,
  isolation: { type: "CallerProvided", path: "/repo" },
  verifyCommands: ["x test"],
  gate: "custom-gate",
  allowPartialCommit: false,
};

test("answerDelegation resumes with --resume and original run context", async () => {
  const { registry, last } = registryWithLookup(() => ({
    ok: true,
    sessionId: "sess-9",
    resumeContext: parkedCtx,
  }));
  const deps = depsWith(registry, ["rm -rf /"]);
  const res = await answerDelegation("job-1", "use v2", deps);
  assert.equal((res as { status: string }).status, "RUNNING");
  const spec = last();
  assert.ok(spec.argv.includes("--resume"));
  assert.equal(spec.argv[spec.argv.indexOf("--resume") + 1], "sess-9");
  assert.equal(spec.argv[spec.argv.length - 1].includes("use v2"), true);
  assert.equal(spec.model, "grok-4.5-xhigh");
  assert.equal(spec.isWrite, true);
  assert.equal(spec.cwd, "/repo");
  assert.equal(spec.path, "/repo");
  assert.equal(spec.gate, "custom-gate");
  assert.deepEqual(spec.resumeContext.isolation, {
    type: "CallerProvided",
    path: "/repo",
  });
  assert.deepEqual(spec.resumeContext.verifyCommands, ["x test"]);
});

test("answerDelegation returns NOT_FOUND for unknown jobId", async () => {
  const { registry } = registryWithLookup(() => ({
    ok: false,
    error: "NOT_FOUND",
  }));
  const res = await answerDelegation("missing", "x", depsWith(registry, []));
  assert.deepEqual(res, { status: "NOT_FOUND" });
});

test("answerDelegation rejects a job that is not awaiting an answer", async () => {
  const { registry } = registryWithLookup(() => ({
    ok: false,
    error: "NOT_AWAITING",
    status: "DONE",
  }));
  await assert.rejects(
    () => answerDelegation("job-done", "x", depsWith(registry, [])),
    /job is not awaiting an answer/,
  );
});
