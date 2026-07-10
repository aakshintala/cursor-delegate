# Needs-Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make mid-run clarifying questions a first-class round-trip: instruct delegates to emit `STATUS: NEEDS_CONTEXT`, retain every such run as a parked job with `jobId` + resume context, and resume via a new `cursor_answer(jobId, answer)` MCP tool.

**Architecture:** Reuse the existing `NEEDS_CONTEXT` status and trailing `STATUS:` parser. Inject a fixed status-convention block into every composed prompt. Persist a `ResumeContext` on every `JobSpec` so a completed `NEEDS_CONTEXT` job can be looked up and resumed. `cursor_answer` looks up the parked job, rejects non-awaiting jobs, and re-enters `runDelegation` with `--resume <sessionId>` and the caller's `answer` as the prompt, reusing the original isolation/capability/verifyCommands/gate/model.

**Tech Stack:** TypeScript (NodeNext ESM), Node.js built-in test runner (`node --import tsx --test`), `@modelcontextprotocol/sdk`, existing job registry + `runDelegation` pipeline.

## Global Constraints

- Signal: reuse the existing `NEEDS_CONTEXT` status (already in the vocab, currently unused). The delegated agent is instructed (prompt convention, same mechanism as today's trailing `STATUS:` line parsed in `output.ts`) to end with `STATUS: NEEDS_CONTEXT` when it needs an answer to proceed. Its final message *is* the question — surfaced in the existing `text` field. No new payload field.
- Uniform `jobId`: any run ending in `NEEDS_CONTEXT` is registered/retained as a **parked job** (with its `sessionId` + original run context), so the result always carries a `jobId` — even a foreground run that never detached for slowness. This makes `cursor_answer(jobId, …)` the single resume path for both foreground and backgrounded/fanned-out needs-input, with no `sessionId`-only special case.
- Resume: new **`cursor_answer(jobId, answer)`** tool. Looks up the parked job in the registry, retrieves its `sessionId` and the original run context (isolation, capability, verifyCommands, gate), and resumes via `--resume <sessionId>` with `answer` as the prompt. Returns the same shape as `cursor_run` (may be terminal, may be `NEEDS_CONTEXT` again, may detach to `RUNNING`/`jobId`). Errors: unknown/expired `jobId` → `NOT_FOUND`; a job not awaiting input → rejected ("job is not awaiting an answer").
- Reliability caveat: detection leans on the model emitting the `STATUS:` line. A weaker model may skip it and just guess; the orchestrator's review of the result is the backstop. This is acceptable for the driving use case (every delegated plan is reviewed). ACP's typed `ask_question` would remove this reliance — recorded as part of the ACP trigger (§9).
- Out of scope for this plan: model layer (§4–5), `doctor` (§7), `delegate` skill + catalog (§8). Assume the model-layer plan is already merged.

## File structure (target)

| Path | Role |
|------|------|
| `src/prompt.ts` | Add `statusBlock()`; include it in `composePrompt` |
| `src/types.ts` | Add `ResumeContext`; add `resumeContext` to `JobSpec` |
| `src/runner.ts` | Populate `resumeContext` on every `JobSpec`; add `answerDelegation` |
| `src/job-registry.ts` | Add `lookupAnswer(jobId)` for parked `NEEDS_CONTEXT` jobs |
| `src/tool-schemas.ts` | Add `cursor_answer` to `buildTools` |
| `src/index.ts` | Dispatch `cursor_answer` → `answerDelegation` |
| `tests/helpers.ts` | `fakeFinalize` uses `deriveStatus`; `specOf` includes `resumeContext` |
| `tests/prompt.test.ts` | Status-convention composition tests |
| `tests/runner.test.ts` | `resumeContext` capture + `answerDelegation` tests |
| `tests/job-registry.test.ts` | Parked-job lookup + uniform `jobId` tests |
| `tests/tool-schemas.test.ts` | Seven-tool list includes `cursor_answer` |
| `tests/index.test.ts` | Route `cursor_answer`; tool-count assertion |

**Assumed already true (model-layer end-state):** `config/models.json` exists; `src/models.ts` exports `resolveModel` / `ModelNotAllowedError` / `NonClaudeViolationError`; `src/tiers.ts` is deleted; `src/tool-schemas.ts` exports `buildTools(config)` / `buildRunInputSchema(config)`; `RunInput` has `model?` + `requireNonClaude?` and no `tier`; `Config` is `{ default, models, priceMap, profile }`.

---

### Task 1: Inject the `STATUS: NEEDS_CONTEXT` prompt convention

**Files:**
- Modify: `src/prompt.ts:1-32`
- Test: `tests/prompt.test.ts`

**Interfaces:**
- Consumes: existing `composePrompt` / `verifyBlock`
- Produces:
  - `statusBlock(): string` — fixed instruction telling the agent to end with a trailing `STATUS:` line, and to use `STATUS: NEEDS_CONTEXT` when it needs an orchestrator answer (question body = message text)
  - `composePrompt` join order becomes: `[preamble?, verifyBlock?, statusBlock(), prompt]` joined by `"\n\n---\n\n"`, then NUL-strip

- [ ] **Step 1: Write the failing prompt tests**

Append to `tests/prompt.test.ts`:

```typescript
test("statusBlock instructs NEEDS_CONTEXT for mid-run questions", () => {
  const b = statusBlock();
  assert.match(b, /STATUS: NEEDS_CONTEXT/);
  assert.match(b, /STATUS: DONE/);
  assert.match(b, /question/i);
});

test("compose always includes the status convention block", () => {
  const out = composePrompt({ prompt: "do the thing" });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 2);
  assert.match(parts[0], /STATUS: NEEDS_CONTEXT/);
  assert.equal(parts[1], "do the thing");
});

test("compose joins preamble + verify + status + prompt", () => {
  const out = composePrompt({
    preamble: "STANDING",
    verifyCommands: ["x test"],
    prompt: "do the thing",
  });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 4);
  assert.equal(parts[0], "STANDING");
  assert.match(parts[1], /ONLY verification/);
  assert.match(parts[2], /STATUS: NEEDS_CONTEXT/);
  assert.equal(parts[3], "do the thing");
});
```

Update the existing import line to:

```typescript
import { composePrompt, verifyBlock, statusBlock } from "../src/prompt.js";
```

Replace the existing three-part composition test with the four-part test above (already included in the append block — delete the old `"compose joins preamble + verify + prompt with the separator"` test so only the four-part version remains).

Replace `"compose omits empty preamble and verify"` (status block is never empty, so the output is no longer just `"hi"`):

```typescript
test("compose omits empty preamble and verify but keeps status block", () => {
  const out = composePrompt({ prompt: "hi" });
  const parts = out.split("\n\n---\n\n");
  assert.equal(parts.length, 2);
  assert.match(parts[0], /STATUS:/);
  assert.equal(parts[1], "hi");
});
```
- [ ] **Step 2: Run prompt tests — expect FAIL**

Run: `node --import tsx --test tests/prompt.test.ts`

Expected: FAIL with `statusBlock` not exported / `is not a function`, or assertion failures on part counts (3 vs 2/4).

- [ ] **Step 3: Implement `statusBlock` and wire it into `composePrompt`**

Replace `src/prompt.ts` with:

```typescript
const SEP = "\n\n---\n\n";

/**
 * Render the verify-scope block (#5): the ONLY verification commands the agent may run.
 * Returns null when there are no verify commands.
 */
export function verifyBlock(verifyCommands?: string[]): string | null {
  if (!verifyCommands || verifyCommands.length === 0) return null;
  const list = verifyCommands.map((c) => `\`${c}\``).join(", ");
  return (
    `These are the ONLY verification commands you may run: ${list}. ` +
    "Do not run workspace-wide builds (e.g. `cargo check --workspace`, full test suites) " +
    "or any other build/test command."
  );
}

/**
 * Standing status convention: trailing STATUS line, including NEEDS_CONTEXT for
 * mid-run questions (question body = message text; no extra payload field).
 */
export function statusBlock(): string {
  return (
    "End your final message with a single trailing line that is exactly one of: " +
    "STATUS: DONE, STATUS: DONE_WITH_CONCERNS, STATUS: BLOCKED, STATUS: NEEDS_CONTEXT, or STATUS: ERROR. " +
    "When you need an answer from the orchestrator before you can proceed, put your question in the " +
    "message body and end with STATUS: NEEDS_CONTEXT."
  );
}

/**
 * Compose the final prompt: [preamble?, verifyBlock?, statusBlock(), prompt] joined by
 * "\n\n---\n\n", then strip all NUL bytes (#1 — spawn throws on a NUL in any argv entry).
 */
export function composePrompt(opts: {
  preamble?: string;
  verifyCommands?: string[];
  prompt: string;
}): string {
  const parts = [
    opts.preamble,
    verifyBlock(opts.verifyCommands),
    statusBlock(),
    opts.prompt,
  ].filter((p): p is string => typeof p === "string" && p.length > 0);
  return parts.join(SEP).replace(/\0/g, "");
}
```

- [ ] **Step 4: Run prompt tests — expect PASS**

Run: `node --import tsx --test tests/prompt.test.ts`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/prompt.ts tests/prompt.test.ts
git commit -m "$(cat <<'EOF'
feat(prompt): instruct delegates to emit STATUS: NEEDS_CONTEXT

EOF
)"
```

---

### Task 2: Add `ResumeContext` and attach it to every `JobSpec`

**Files:**
- Modify: `src/types.ts` (`JobSpec` interface; add `ResumeContext` near it)
- Modify: `src/runner.ts` (`runDelegation` JobSpec construction)
- Modify: `tests/helpers.ts` (`specOf`)
- Test: `tests/runner.test.ts`

**Interfaces:**
- Consumes: `RunInput`, `Capability`, `Isolation` from `src/types.ts`; `runDelegation` from Task 1's prompt wiring (unchanged call shape)
- Produces:
  - `ResumeContext`:
    ```typescript
    export interface ResumeContext {
      model: string;
      requireNonClaude?: boolean;
      capability: Capability;
      allowUnsandboxed: boolean;
      isolation: Isolation;
      verifyCommands?: string[];
      gate: string;
      allowPartialCommit: boolean;
    }
    ```
  - `JobSpec.resumeContext: ResumeContext` (required on every spec)
  - `runDelegation` populates `resumeContext` from the resolved model + input fields used for resume

- [ ] **Step 1: Write the failing runner test for resume-context capture**

Append to `tests/runner.test.ts` (using the post–model-layer `Config` fixture already present):

```typescript
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
    depsWith(registry, []),
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
  await runDelegation({ prompt: "do it" }, depsWith(registry, []));
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
```

- [ ] **Step 2: Run runner tests — expect FAIL**

Run: `node --import tsx --test tests/runner.test.ts`

Expected: FAIL with `resumeContext` undefined / missing property on `JobSpec`.

- [ ] **Step 3: Add `ResumeContext` and `JobSpec.resumeContext`**

In `src/types.ts`, add immediately above `JobSpec`:

```typescript
/** Fields needed to resume a parked NEEDS_CONTEXT job via cursor_answer. */
export interface ResumeContext {
  model: string;
  requireNonClaude?: boolean;
  capability: Capability;
  allowUnsandboxed: boolean;
  isolation: Isolation;
  verifyCommands?: string[];
  gate: string;
  allowPartialCommit: boolean;
}
```

Add `resumeContext: ResumeContext;` to the `JobSpec` interface (after `worktreeName?: string` is fine):

```typescript
export interface JobSpec {
  bin: string;
  argv: string[];
  cwd: string;
  model: string;
  backend: string;
  isWrite: boolean;
  path: string | null; // CallerProvided path for the write lock, else null
  headBefore: string | null;
  gate: string; // "" = no gate
  allowPartialCommit: boolean;
  waitMs?: number;
  background?: boolean;
  priceMap: PriceMap;
  downgraded: boolean;
  /** Set for BackendProvided isolation; used to resolve the worktree for change-set tracking. */
  worktreeName?: string;
  /** Original run knobs for cursor_answer resume. */
  resumeContext: ResumeContext;
}
```

- [ ] **Step 4: Populate `resumeContext` in `runDelegation` and fix `specOf`**

In `src/runner.ts`, inside `runDelegation`, when building `spec`, add:

```typescript
  const capability = input.capability ?? "ask";
  const allowUnsandboxed = input.allowUnsandboxed ?? false;
  const isolation = input.isolation ?? { type: "None" as const };
  const allowPartialCommit = input.allowPartialCommit ?? false;

  const resumeContext: ResumeContext = {
    model: resolved.model,
    capability,
    allowUnsandboxed,
    isolation,
    gate,
    allowPartialCommit,
  };
  if (input.requireNonClaude !== undefined) {
    resumeContext.requireNonClaude = input.requireNonClaude;
  }
  if (input.verifyCommands !== undefined) {
    resumeContext.verifyCommands = input.verifyCommands;
  }
```

Use those locals for the existing `mapCapability` / `mapIsolation` / `allowPartialCommit` call sites so values stay consistent (replace `input.capability ?? "ask"` with `capability`, etc.).

Add `resumeContext` to the `JobSpec` literal.

Import `ResumeContext` from `./types.js`.

In `tests/helpers.ts`, update `specOf` to include a default resume context:

```typescript
export function specOf(over: Partial<JobSpec> = {}): JobSpec {
  return {
    bin: "cursor-agent",
    argv: ["--print"],
    cwd: "/tmp",
    model: "composer-2.5",
    backend: "cursor",
    isWrite: false,
    path: null,
    headBefore: null,
    gate: "",
    allowPartialCommit: false,
    priceMap: {},
    downgraded: false,
    resumeContext: {
      model: "composer-2.5",
      capability: "ask",
      allowUnsandboxed: false,
      isolation: { type: "None" },
      gate: "",
      allowPartialCommit: false,
    },
    ...over,
  };
}
```

- [ ] **Step 5: Run runner tests — expect PASS**

Run: `node --import tsx --test tests/runner.test.ts`

Expected: PASS.

Also run registry tests (they use `specOf`):

Run: `node --import tsx --test tests/job-registry.test.ts`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/types.ts src/runner.ts tests/helpers.ts tests/runner.test.ts
git commit -m "$(cat <<'EOF'
feat(jobs): persist ResumeContext on every JobSpec for answer resume

EOF
)"
```

---

### Task 3: Park `NEEDS_CONTEXT` jobs and expose `lookupAnswer`

**Files:**
- Modify: `src/job-registry.ts` (`JobRegistry` interface + `makeJobRegistry` implementation)
- Modify: `tests/helpers.ts` (`fakeFinalize` → use `deriveStatus`)
- Test: `tests/job-registry.test.ts`

**Interfaces:**
- Consumes: `JobStatus`, `ResumeContext`, `RunOutput` from `src/types.ts`; completed-job retention already in `retire`
- Produces:
  - `AnswerLookup`:
    ```typescript
    export type AnswerLookup =
      | { ok: true; sessionId: string; resumeContext: ResumeContext }
      | { ok: false; error: "NOT_FOUND" }
      | { ok: false; error: "NOT_AWAITING"; status: JobStatus };
    ```
  - `JobRegistry.lookupAnswer(jobId: string): AnswerLookup`
  - Semantics:
    - unknown / evicted (expired) id → `{ ok: false, error: "NOT_FOUND" }`
    - found but `status !== "NEEDS_CONTEXT"` OR missing `terminalOutput.sessionId` → `{ ok: false, error: "NOT_AWAITING", status }`
    - found, `NEEDS_CONTEXT`, non-null `sessionId` → `{ ok: true, sessionId, resumeContext: job.spec.resumeContext }`
  - Foreground `NEEDS_CONTEXT` results still carry `jobId` (already set in `finalizeJob`; assert it)

- [ ] **Step 1: Update `fakeFinalize` so registry tests can emit `NEEDS_CONTEXT`**

In `tests/helpers.ts`, replace `fakeFinalize` with:

```typescript
import { deriveStatus } from "../src/output.js";

/** A finalize that builds a RunOutput from the raw result without touching git/gate. */
export const fakeFinalize = async (
  res: BackendResult,
  ctx: { backend: string; model: string; jobId?: string },
) => ({
  status: deriveStatus(res.raw.result ?? "", res.raw.is_error, res.cleanExit),
  text: res.raw.result ?? "",
  sessionId: res.raw.session_id ?? null,
  backend: ctx.backend,
  model: ctx.model,
  usage: res.raw.usage ?? null,
  costUsd: null,
  costEstimated: true,
  durationMs: res.raw.duration_ms ?? null,
  jobId: ctx.jobId,
});
```

Keep the import of `BackendResult` / `JobSpec` as they are; add the `deriveStatus` import at the top.

- [ ] **Step 2: Write the failing registry tests**

Append to `tests/job-registry.test.ts`:

```typescript
const needsContextOk: BackendResult = {
  raw: {
    result: "Which API version should I target?\nSTATUS: NEEDS_CONTEXT",
    is_error: false,
    session_id: "sess-park-1",
  },
  cleanExit: true,
  stderr: "",
};

test("foreground NEEDS_CONTEXT returns RunOutput with jobId and parks the job", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf());
  handles[0].finish(needsContextOk);
  const res = (await p) as RunOutput;
  assert.equal(res.status, "NEEDS_CONTEXT");
  assert.ok(typeof res.jobId === "string" && res.jobId.length > 0);
  assert.equal(res.sessionId, "sess-park-1");
  assert.match(res.text, /Which API version/);

  const looked = registry.lookupAnswer(res.jobId!);
  assert.equal(looked.ok, true);
  if (looked.ok) {
    assert.equal(looked.sessionId, "sess-park-1");
    assert.equal(looked.resumeContext.model, "composer-2.5");
  }
});

test("lookupAnswer returns NOT_FOUND for unknown or expired ids", () => {
  const { registry } = setup();
  assert.deepEqual(registry.lookupAnswer("nope"), {
    ok: false,
    error: "NOT_FOUND",
  });
});

test("lookupAnswer rejects jobs that are not awaiting an answer", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf({ background: true }));
  const id = jobId(await p);
  assert.deepEqual(registry.lookupAnswer(id), {
    ok: false,
    error: "NOT_AWAITING",
    status: "RUNNING",
  });

  handles[0].finish(doneOk);
  await flush();
  assert.deepEqual(registry.lookupAnswer(id), {
    ok: false,
    error: "NOT_AWAITING",
    status: "DONE",
  });
});

test("lookupAnswer rejects NEEDS_CONTEXT jobs that lack a sessionId", async () => {
  const { registry, handles } = setup();
  const p = registry.dispatch(specOf());
  handles[0].finish({
    raw: {
      result: "Need a choice\nSTATUS: NEEDS_CONTEXT",
      is_error: false,
    },
    cleanExit: true,
    stderr: "",
  });
  const res = (await p) as RunOutput;
  assert.equal(res.status, "NEEDS_CONTEXT");
  assert.deepEqual(registry.lookupAnswer(res.jobId!), {
    ok: false,
    error: "NOT_AWAITING",
    status: "NEEDS_CONTEXT",
  });
});
```

- [ ] **Step 3: Run registry tests — expect FAIL**

Run: `node --import tsx --test tests/job-registry.test.ts`

Expected: FAIL with `lookupAnswer is not a function` (and/or `fakeFinalize` status still not `NEEDS_CONTEXT` if Step 1 was skipped).

- [ ] **Step 4: Implement `lookupAnswer` on the registry**

In `src/job-registry.ts`:

1. Import `ResumeContext` and `JobStatus` (JobStatus already imported via types — ensure `ResumeContext` is imported):

```typescript
import type {
  DispatchResult,
  FinalizeCtx,
  JobSpec,
  JobStatus,
  PollResult,
  ResumeContext,
  RunOutput,
} from "./types.js";
```

2. Add the exported type above `JobRegistry`:

```typescript
export type AnswerLookup =
  | { ok: true; sessionId: string; resumeContext: ResumeContext }
  | { ok: false; error: "NOT_FOUND" }
  | { ok: false; error: "NOT_AWAITING"; status: JobStatus };
```

3. Add to the `JobRegistry` interface:

```typescript
  lookupAnswer(jobId: string): AnswerLookup;
```

4. Implement inside `makeJobRegistry`, next to `poll`:

```typescript
  function lookupAnswer(jobId: string): AnswerLookup {
    const job = getJob(jobId);
    if (!job) return { ok: false, error: "NOT_FOUND" };
    const sessionId = job.terminalOutput?.sessionId ?? null;
    if (job.status !== "NEEDS_CONTEXT" || !sessionId) {
      return { ok: false, error: "NOT_AWAITING", status: job.status };
    }
    return {
      ok: true,
      sessionId,
      resumeContext: job.spec.resumeContext,
    };
  }
```

5. Include `lookupAnswer` in the returned object:

```typescript
  return { dispatch, poll, cancel, wait, waitAny, waitAll, killAll, lookupAnswer };
```

No separate parked map: a job that finishes with `NEEDS_CONTEXT` is already moved to `completed` by `retire`, retains `spec.resumeContext` + `terminalOutput.sessionId`, and is evicted under `COMPLETED_CAP` (expired → `NOT_FOUND`).

- [ ] **Step 5: Run registry tests — expect PASS**

Run: `node --import tsx --test tests/job-registry.test.ts`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/job-registry.ts tests/helpers.ts tests/job-registry.test.ts
git commit -m "$(cat <<'EOF'
feat(registry): lookup parked NEEDS_CONTEXT jobs for cursor_answer

EOF
)"
```

---

### Task 4: `answerDelegation` resumes via `--resume` with original context

**Files:**
- Modify: `src/runner.ts` (add `answerDelegation`)
- Test: `tests/runner.test.ts`

**Interfaces:**
- Consumes:
  - `JobRegistry.lookupAnswer(jobId): AnswerLookup` from Task 3
  - `runDelegation(input, deps, opts): Promise<DispatchResult>` (existing)
  - `ResumeContext` from Task 2
- Produces:
  - `answerDelegation(jobId: string, answer: string, deps: RunnerDeps, opts?: DispatchOpts): Promise<DispatchResult | { status: "NOT_FOUND" }>`
  - On `NOT_FOUND` → return `{ status: "NOT_FOUND" }`
  - On `NOT_AWAITING` → `throw new Error("job is not awaiting an answer")`
  - On success → `runDelegation` with:
    - `prompt: answer`
    - `session: sessionId` (drives `buildArgv` `--resume`)
    - `model`, `requireNonClaude`, `capability`, `allowUnsandboxed`, `isolation`, `verifyCommands`, `gate`, `allowPartialCommit` from `resumeContext`
  - Does not forward original `waitMs` / `background` (YAGNI; `cursor_answer` schema has only `jobId` + `answer`)

- [ ] **Step 1: Write the failing `answerDelegation` tests**

At the top of `tests/runner.test.ts`, update imports to:

```typescript
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
```

(Keep the existing post–model-layer `config` fixture and `fakeRegistry` / `depsWith` helpers.)

Then append below the existing tests:

```typescript
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
```

- [ ] **Step 2: Run runner tests — expect FAIL**

Run: `node --import tsx --test tests/runner.test.ts`

Expected: FAIL with `answerDelegation` not exported / `is not a function`.

- [ ] **Step 3: Implement `answerDelegation` in `src/runner.ts`**

Add after `runDelegation`:

```typescript
/**
 * Resume a parked NEEDS_CONTEXT job: look up sessionId + ResumeContext, then
 * re-enter runDelegation with answer as the prompt and --resume <sessionId>.
 */
export async function answerDelegation(
  jobId: string,
  answer: string,
  deps: RunnerDeps,
  opts: DispatchOpts = {},
): Promise<DispatchResult | { status: "NOT_FOUND" }> {
  const looked = deps.registry.lookupAnswer(jobId);
  if (!looked.ok) {
    if (looked.error === "NOT_FOUND") return { status: "NOT_FOUND" };
    throw new Error("job is not awaiting an answer");
  }

  const ctx = looked.resumeContext;
  return runDelegation(
    {
      prompt: answer,
      session: looked.sessionId,
      model: ctx.model,
      requireNonClaude: ctx.requireNonClaude,
      capability: ctx.capability,
      allowUnsandboxed: ctx.allowUnsandboxed,
      isolation: ctx.isolation,
      verifyCommands: ctx.verifyCommands,
      // Resolved gate string from the original run ("" = no gate). `??` in
      // runDelegation preserves "" and does not re-apply the profile default.
      gate: ctx.gate,
      allowPartialCommit: ctx.allowPartialCommit,
    },
    deps,
    opts,
  );
}
```

- [ ] **Step 4: Run runner tests — expect PASS**

Run: `node --import tsx --test tests/runner.test.ts`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/runner.ts tests/runner.test.ts
git commit -m "$(cat <<'EOF'
feat(runner): resume parked jobs via answerDelegation and --resume

EOF
)"
```

---

### Task 5: Expose `cursor_answer` MCP tool and wire `handleCall`

**Files:**
- Modify: `src/tool-schemas.ts` (`buildTools` — add seventh tool)
- Modify: `src/index.ts` (`handleCall` switch; import `answerDelegation`)
- Modify: `tests/tool-schemas.test.ts` (tool list length/names)
- Modify: `tests/index.test.ts` (routing + fake registry `lookupAnswer`)
- Test: `tests/tool-schemas.test.ts`, `tests/index.test.ts`

**Interfaces:**
- Consumes: `answerDelegation` from Task 4; `buildTools(config)` from model-layer end-state
- Produces:
  - MCP tool `cursor_answer` with input `{ jobId: string; answer: string }` (both required)
  - `handleCall("cursor_answer", …)` → `answerDelegation(jobId, answer, deps, { sink, signal })`
  - Missing `jobId` / `answer` → throw via existing `requireString`
  - Tool list length becomes 7

- [ ] **Step 1: Write the failing tool-schema + index tests**

In `tests/tool-schemas.test.ts`, update the `buildTools` assertions from six tools to seven. Replace the tools-name expectation with:

```typescript
test("buildTools wires cursor_run description with blurb and default", () => {
  const tools = buildTools(config);
  assert.equal(tools.length, 7);
  const run = tools.find((t) => t.name === "cursor_run");
  assert.ok(run);
  assert.match(run!.description, /composer-2\.5 — Composer 2\.5/);
  assert.match(run!.description, /Default model: composer-2\.5/);
  assert.equal(
    (run!.inputSchema as { properties: { model: { enum: string[] } } })
      .properties.model.enum.includes("grok-4.5-xhigh"),
    true,
  );
  assert.deepEqual(
    tools.map((t) => t.name),
    [
      "cursor_run",
      "cursor_poll",
      "cursor_cancel",
      "cursor_wait",
      "cursor_wait_any",
      "cursor_wait_all",
      "cursor_answer",
    ],
  );
});

test("cursor_answer schema requires jobId and answer", () => {
  const tools = buildTools(config);
  const answer = tools.find((t) => t.name === "cursor_answer");
  assert.ok(answer);
  const schema = answer!.inputSchema as {
    required: string[];
    properties: { jobId: { type: string }; answer: { type: string } };
  };
  assert.deepEqual(schema.required, ["jobId", "answer"]);
  assert.equal(schema.properties.jobId.type, "string");
  assert.equal(schema.properties.answer.type, "string");
  assert.match(answer!.description, /NEEDS_CONTEXT/);
});
```

In `tests/index.test.ts`:

1. Extend `fakeRegistry` with `lookupAnswer` (unused by most tests, but keeps the fake complete):

```typescript
    lookupAnswer: (id: string) => {
      calls.push(`lookupAnswer:${id}`);
      return { ok: false as const, error: "NOT_FOUND" as const };
    },
```

2. Replace the six-tools test with:

```typescript
test("exposes seven tools from buildTools", () => {
  const tools = buildTools(config);
  assert.equal(tools.length, 7);
  const names = tools.map((t) => t.name);
  assert.deepEqual(names, [
    "cursor_run",
    "cursor_poll",
    "cursor_cancel",
    "cursor_wait",
    "cursor_wait_any",
    "cursor_wait_all",
    "cursor_answer",
  ]);
});
```

3. Replace the existing `"routes each tool to the registry"` test with:

```typescript
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
```

Note: with the default fake `lookupAnswer` returning `NOT_FOUND`, `cursor_answer` returns `{ status: "NOT_FOUND" }` and does not call `dispatch`. That is correct for this routing test. Happy-path resume (lookup → `runDelegation` with `--resume`) is covered in `tests/runner.test.ts` Task 4.

4. Append these `cursor_answer` handler tests (no live bin required):

```typescript
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
```

- [ ] **Step 2: Run schema + index tests — expect FAIL**

Run: `node --import tsx --test tests/tool-schemas.test.ts tests/index.test.ts`

Expected: FAIL — tools length still 6 / unknown tool `"cursor_answer"` / `lookupAnswer` missing on fake.

- [ ] **Step 3: Add `cursor_answer` to `buildTools`**

In `src/tool-schemas.ts`, add a schema constant next to the other job schemas:

```typescript
const ANSWER_SCHEMA = {
  type: "object",
  properties: {
    jobId: {
      type: "string",
      description:
        "jobId of a parked run that ended with status NEEDS_CONTEXT.",
    },
    answer: {
      type: "string",
      description:
        "Orchestrator answer to the delegated agent's question (becomes the resume prompt).",
    },
  },
  required: ["jobId", "answer"],
} as const;
```

Append this tool object to the array returned by `buildTools` (after `cursor_wait_all`):

```typescript
    {
      name: "cursor_answer",
      description:
        "Resume a parked NEEDS_CONTEXT job: look up its sessionId and original run context " +
        "(isolation, capability, verifyCommands, gate, model), then continue via " +
        "--resume <sessionId> with `answer` as the prompt. Returns the same shape as " +
        "cursor_run (terminal, NEEDS_CONTEXT again, or RUNNING/jobId). " +
        "Unknown/expired jobId → {status:'NOT_FOUND'}; a job not awaiting input is rejected.",
      inputSchema: ANSWER_SCHEMA,
    },
```

Update the file's top comment from "six MCP tools" to "MCP tools".

- [ ] **Step 4: Wire `handleCall` in `src/index.ts`**

1. Change the runner import:

```typescript
import { runDelegation, answerDelegation } from "./runner.js";
```

2. Add a case before `default`:

```typescript
    case "cursor_answer":
      return answerDelegation(
        requireString(args, "jobId"),
        requireString(args, "answer"),
        deps,
        { sink, signal },
      );
```

- [ ] **Step 5: Run schema + index tests — expect PASS**

Run: `node --import tsx --test tests/tool-schemas.test.ts tests/index.test.ts`

Expected: PASS.

- [ ] **Step 6: Run the full needs-input unit slice — expect PASS**

Run:

```bash
node --import tsx --test \
  tests/prompt.test.ts \
  tests/output.test.ts \
  tests/job-registry.test.ts \
  tests/runner.test.ts \
  tests/tool-schemas.test.ts \
  tests/index.test.ts
```

Expected: PASS. (`tests/output.test.ts` already covers `NEEDS_CONTEXT` parsing from the trailing `STATUS:` line — no code change required there.)

- [ ] **Step 7: Commit**

```bash
git add src/tool-schemas.ts src/index.ts tests/tool-schemas.test.ts tests/index.test.ts
git commit -m "$(cat <<'EOF'
feat(mcp): add cursor_answer tool to resume parked NEEDS_CONTEXT jobs

EOF
)"
```

---

## Self-Review

### 1. Spec coverage (§6 + relevant §11)

| Spec item | Task |
|-----------|------|
| Reuse `NEEDS_CONTEXT` status (no new status / payload field) | Task 1 (instruction); existing `output.ts` / `tests/output.test.ts` (parse) |
| Prompt convention: end with `STATUS: NEEDS_CONTEXT`; question = `text` | Task 1 (`statusBlock` in `composePrompt`) |
| Uniform `jobId`: foreground + background parked with sessionId + run context | Task 2 (`ResumeContext` on `JobSpec`); Task 3 (foreground assert + `lookupAnswer`) |
| No `sessionId`-only special-case resume path | Task 4–5 (`cursor_answer(jobId, …)` only) |
| `cursor_answer(jobId, answer)` resumes via `--resume` + original isolation/capability/verifyCommands/gate | Task 4 (`answerDelegation`); Task 5 (MCP wire-up) |
| Same return shape as `cursor_run` | Task 4 (returns `runDelegation` result) |
| Unknown/expired `jobId` → `NOT_FOUND` | Task 3–5 (`COMPLETED_CAP` eviction = expired) |
| Job not awaiting input → rejected `"job is not awaiting an answer"` | Task 3–5 |
| §11: `NEEDS_CONTEXT` parsed from `STATUS:` line | Already green in `tests/output.test.ts`; exercised again in Task 3 |
| §11: `cursor_answer` happy-path / NOT_FOUND / not-awaiting | Tasks 4–5 |
| Reliability caveat / ACP deferral | Documented in Global Constraints; no code (out of scope) |
| §4–5 model layer, §7 doctor, §8 delegate skill | Explicitly excluded — no tasks |

### 2. Placeholder scan

No TBD/TODO steps. Every code step includes full source. Every command includes the exact invocation. Types/functions referenced later (`ResumeContext`, `AnswerLookup`, `lookupAnswer`, `answerDelegation`, `statusBlock`, `cursor_answer`) are defined in an earlier task.

### 3. Type consistency

- `ResumeContext` fields match what `answerDelegation` passes into `RunInput` (`model`, `requireNonClaude?`, `capability`, `allowUnsandboxed`, `isolation`, `verifyCommands?`, `gate`, `allowPartialCommit`).
- `AnswerLookup` discriminant is `ok: true | false` with `error: "NOT_FOUND" | "NOT_AWAITING"` — used identically in registry, runner, and index tests.
- Rejection message is exactly `job is not awaiting an answer` in runner throw and index/runner tests.
- `JobSpec.resumeContext` is required; `specOf` and `runDelegation` both populate it.
- Tool name is `cursor_answer` everywhere (schema, `handleCall`, tests).
- Post–model-layer assumptions hold: `buildTools(config)`, `Config = { default, models, priceMap, profile }`, `resolveModel` from `src/models.ts`, no `tier`.
