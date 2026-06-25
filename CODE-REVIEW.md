# Code Review — cursor-delegate

**Date:** 2026-06-25
**Scope:** Full `src/` tree (~2,000 LOC, 22 files). No git repo present, so this is a whole-tree audit rather than a diff review.
**Method:** 4 parallel finder angles (line-by-line correctness, cross-file tracer, cleanup, altitude) + manual verification against source. No `CLAUDE.md` exists, so no convention checks apply.

Findings ranked most-severe first. Correctness ranks above cleanup.

---

## Correctness

### 1. Idle/stall timer not cleared at child close → successful job reported as `STALLED`
**`src/job-registry.ts:106,162-167`**

The idle timer is only cleared in `retire()`, which runs *after* `finalizeJob` → `finalize()` completes. `finalize` awaits `runGate` (an arbitrary shell command) plus several `git` calls. The timer fires `idleMs` after the last *stream* event, so if finalize outlasts that window (e.g. a slow gate command), the idle callback at line 163 still sees `status === "RUNNING"`, sets `terminationReason = "STALLED"`, and kills the (already-dead) child. Then `retire()` does `job.status = job.terminationReason ?? out.status` — a job that actually returned `DONE` is surfaced to the caller as `STALLED`.

**Fix:** Clear the idle timer when the child closes (in the backend `done`/`close` path or at the top of `finalizeJob`), before finalize runs.

---

### 2. `BackendProvided` (worktree) isolation computes the change-set in the wrong directory
**`src/isolation.ts:24-29`, `src/finalize.ts:47`, `src/runner.ts:78`**

`mapIsolation` returns `cwd: serverCwd` for worktree isolation, but `cursor-agent --worktree` works in a *separate* directory. `captureHead` and `gitDelta` both run against `serverCwd`, where the agent's commits/edits never landed. Result: `changeSet` is empty/misleading and the incomplete-commit concern (#1 invariant) can never fire for worktree jobs — even though the tool advertises a change-set for every run.

**Fix:** Resolve the actual worktree path and use it as `cwd` for head capture and `gitDelta`, or document that change-set tracking is unavailable for `BackendProvided`.

---

### 3. Prompt appended as a bare positional with no `--` terminator
**`src/runner.ts:41`**

`buildArgv` pushes `o.prompt` directly after the flags. A prompt starting with a dash (`"--help"`, `"--version"`, `"-h fix the bug"`) is parsed by `cursor-agent` as an option instead of the task — agent prints help / errors / does the wrong thing.

**Fix:** Push `"--"` before the prompt positional.

---

### 4. `STATUS:` override matches any line, first-match wins
**`src/output.ts:16`**

`/STATUS:\s*([A-Z_]+)\s*$/m` with `String.match` (no `g`) returns the *first* `STATUS:` line anywhere in the agent's free text, not the trailing one the doc describes. `"...STATUS: NEEDS_CONTEXT\n...\nSTATUS: DONE"` reports `NEEDS_CONTEXT`; incidental prose ending in `STATUS: DONE` flips a failed run to success. (Flagged independently by 3 finder angles.)

**Fix:** Match only the genuine final line (split, take last non-empty line, then test), or make status a first-class field of the agent's structured output contract rather than scraped from prose.

---

### 5. `cursor_run` args enter the core unvalidated
**`src/index.ts:74`**

Only `prompt` is checked; the rest of the object is cast straight to `RunInput`. A non-numeric `waitMs` reaches `clampWait` → `Math.min/max(...NaN...)` → `NaN` → `setTimer(NaN)` fires immediately, so the deadline always wins and every run auto-detaches. `background:"false"` is truthy → always detaches. Note the asymmetry: the wait tools *do* guard `timeoutMs` with `typeof === "number"` (lines 83/89/95), but `waitMs`/`isolation`/`capability`/`background` get nothing.

**Fix:** Validate once at the MCP boundary (the `inputSchema` already exists in `tool-schemas.ts` but is only used to advertise tools — run incoming args through a validator to produce a typed `RunInput` the interior can trust).

---

### 6. Cancelled/stalled job still runs the gate's side effects
**`src/job-registry.ts:193-209,307-313`**

`cancel()` SIGTERMs the child, but the child's `close` still resolves `handle.done`, so `finalizeJob` runs the full `finalize()` — executing the arbitrary `gate` shell command and `gitDelta` — before `retire()` overwrites the status to `CANCELLED` and discards finalize's result. After an explicit cancel, the tool still spends CPU and fires any gate side effects.

**Fix:** Skip finalize (or at least the gate) when `terminationReason` is set.

---

### 7. `filesTouched` only tracks the `edit` tool
**`src/stream.ts:44-48`**

`extractTool` reads `args.path` only when `name === "edit"`; write/create/delete/move tool calls contribute no path, so `filesTouchedSoFar` in poll progress is systematically incomplete. Invisible until someone notices created/deleted files missing.

**Fix:** Extract the path generically from any `*ToolCall` whose args carry a path-shaped field, driven by data rather than a hard-coded tool name.

---

### 8. Live token count can jump backward
**`src/stream.ts:88-92`**

`state.tokensSoFar = usage.outputTokens` on *any* event, with no monotonic guard. If the CLI emits a per-message (non-cumulative) usage block after a larger cumulative value, the polled token count decreases. Low impact (final cost comes from `res.raw.usage`, not this field), but the progress display regresses.

**Fix:** `state.tokensSoFar = Math.max(state.tokensSoFar, n)`.

---

### 9. `busyPath` set on the caller's own still-running job
**`src/job-registry.ts:244-251`**

The deadline-detach path returns `{status:"RUNNING", jobId, busyPath: spec.path}`. The schema (`tool-schemas.ts:122`) documents `busyPath` only for `{status:"BUSY"}` = a *different* job holding the lock. A caller branching on `busyPath` to mean "someone else holds the lock" will misread its own long-running job as a contention conflict.

**Fix:** Omit `busyPath` on the RUNNING-detach result.

---

## Efficiency / Cleanup

### 10. `gitDelta` runs 5 sequential `git` spawns and re-implements `captureHead`
**`src/git.ts:60-78`**

On the finalize hot path (every completed write job) it spawns `rev-parse`, then `rev-list`, `diff --name-only`, `diff --stat`, `status --porcelain` strictly one at a time. The four post-`rev-parse` calls are independent → `Promise.all` cuts latency from ~5× to ~2× spawn round-trips. Separately, lines 60-62 duplicate `captureHead`'s exact `rev-parse HEAD → trim || null` logic.

**Fix:** Call `captureHead(cwd)` for HEAD; run the independent git calls concurrently.

---

## Lower-severity / noted, not fixed now

- **Unbounded buffer growth** — `src/backends/cursor.ts:50,61`: `stderrBuf` (and `stdoutBuf` for a newline-less stream) accumulate for the child's whole lifetime, though finalize keeps only the last 2,048 bytes. Use a rolling tail for long/chatty jobs.
- **Copy-paste in registry** — `src/job-registry.ts`: `raceDeadline` (257) vs `raceTimeout` (317) share identical settle/timer/abort scaffolding; `waitAny` (377) vs `waitAll` (417) duplicate known-set resolution, sink attach/detach, and poll-map assembly. Extract shared primitives so abort/leak fixes apply once.
- **Hard-coded price alias** — `src/pricing.ts:18`: the `gpt-5.5 → gpt-5.5-medium` alias lives inside `computeCost` arithmetic. Resolve the canonical pricing key once (in `resolveModel`/loader) so `computeCost` stays pure.
- **Throws escape the status contract** — `src/runner.ts:56`: `resolveModel`/`verifyDenyList` throw, and the throw propagates uncaught through `handleCall` to the MCP layer as a raw protocol error rather than a structured `BLOCKED`/`ERROR` RunOutput. Decide whether fail-fast-as-MCP-error is intended.
- **`parsePorcelain` fixed 3-char prefix** — `src/git.ts:39`: correct for porcelain v1 (`XY␠path`), but brittle if invoked on other formats; consider `-z` parsing if robustness matters.

---

## Suggested fix order for tomorrow

1. #1, #6 (registry lifecycle: idle-timer clear + skip-finalize-on-cancel — same area)
2. #5 (boundary validation — unblocks trusting types everywhere downstream)
3. #3, #4 (one-line robustness fixes: `--` separator, last-line STATUS match)
4. #2 (worktree change-set directory — needs design decision)
5. #7, #8, #9, #10 (smaller correctness + the hot-path git cleanup)
