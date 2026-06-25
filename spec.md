# cursor-delegate — Reproduction Spec

A self-contained specification for rebuilding the `cursor-delegate` plugin from scratch. It
captures *what* the system does and *how* its pieces fit, precisely enough to reimplement in any
language. The reference implementation is TypeScript/Node on the MCP SDK; where a detail is
language-specific it is called out as such.

---

## 1. What it is

`cursor-delegate` is a **Model Context Protocol (MCP) stdio server** that lets a host agent (Claude
Code, or any MCP client) delegate coding/research tasks to **Cursor's models** by shelling out to
the local `cursor-agent` CLI in headless (`--print`) mode.

It exposes a single generic delegation tool (`cursor_run`) plus a small set of job-control tools
(`cursor_poll`, `cursor_cancel`, `cursor_wait`, `cursor_wait_any`, `cursor_wait_all`). The value it
adds over calling `cursor-agent` directly:

- **Multi-model tiering** — symbolic tiers (`cheap-bulk`, `standard`, `coding-specialist`,
  `diversity`) resolve to concrete model ids via a config map; the caller need not hardcode model
  names. The `diversity` tier is contractually non-Claude (uncorrelated second opinions).
- **Capability modes** — `ask` / `plan` (read-only) vs `write` / `write-unsandboxed`, each mapping
  to a vetted, non-interactive `cursor-agent` flag set.
- **Sandboxing + a fail-closed deny-list** — write calls are refused unless the host's
  `cursor-agent` deny-list contains every required pattern.
- **An async job model** — fast tasks return synchronously; slow tasks detach and hand back a
  `jobId` you poll/wait on. Live progress streams to the client while a call blocks.
- **Ground-truth verification** — the tool itself computes the git change-set over the run, runs an
  optional postcondition "gate" command, and surfaces stderr on failure — rather than trusting the
  agent's self-report.
- **Same-path write serialization** — two concurrent writes to one working tree are refused, not
  interleaved.

It is **local headless only**: it never spawns Cursor *cloud* (`worker`) runs and never passes a
bare `--yolo`.

---

## 2. Architecture & module decomposition

Pure functions for policy; one stateful module (the job registry) for process lifecycle. Every
impure boundary (spawn, git, gate, clock, config read) is injected so it can be faked in tests.

```
MCP client (Claude Code)
   │  stdio (JSON-RPC)
   ▼
index.ts ............ MCP server: tool list, request routing, arg validation,
   │                  progress-sink wiring, shutdown handlers
   ▼
runner.ts ........... runDelegation(): pure pre-flight — resolve model, map capability,
   │                  verify deny-list, map isolation, compose prompt, capture HEAD,
   │                  build a JobSpec, hand to the registry
   ▼
job-registry.ts ..... STATEFUL. spawn via backend, deadline-race, detach+jobId,
   │                  progress tracking, idle watchdog, write-path lock, cancel,
   │                  poll/wait/waitAny/waitAll, shutdown killAll
   ├── backends/cursor.ts ... spawn cursor-agent, parse stream-json (NDJSON) lines,
   │      backends/types.ts    emit "progress"/"stderr" events, resolve a BackendResult
   │   stream.ts ............. incremental stream-json parser (lastTool, tokens, files, phase)
   │   cursor-bin.ts ......... resolve the cursor-agent binary path
   └── finalize.ts ......... assemble terminal RunOutput:
          output.ts ......... base status/usage/text from the raw result blob
          gate.ts ........... run the postcondition command (#7)
          git.ts ............ compute the real change-set (#6)

Policy helpers (pure), consumed by runner.ts:
   tiers.ts ........ tier/model → ResolvedModel (+ diversity-non-Claude contract)
   capability.ts ... capability → cursor-agent flags (+ unsandboxed downgrade)
   isolation.ts .... isolation → {flags, cwd}
   safety.ts ....... fail-closed deny-list verification
   prompt.ts ....... preamble + verify-scope composition + NUL sanitization
   pricing.ts ...... usage × price-map → best-effort USD
   config.ts ....... load + merge default maps with the host profile
   progress.ts ..... ProgressSink + MCP notifications/progress bridge
   types.ts ........ all domain types
```

**Dependency rule:** `runner` and the policy helpers are pure and synchronous-ish (config is async
I/O only). All process/timer state lives in `job-registry`. `index` is the only module that touches
the MCP transport.

---

## 3. Domain types

```ts
type Capability = "ask" | "plan" | "write" | "write-unsandboxed";
type Tier       = "cheap-bulk" | "standard" | "coding-specialist" | "diversity";

type Isolation =
  | { type: "None" }
  | { type: "CallerProvided"; path: string }
  | { type: "BackendProvided"; name?: string; base?: string };

interface RunInput {
  prompt: string;              // required
  tier?: Tier;                 // symbolic model selector
  model?: string;              // raw model id; bypasses tier resolution
  capability?: Capability;     // default "ask"
  allowUnsandboxed?: boolean;  // required 2nd signal for write-unsandboxed
  session?: string;            // resume a prior sessionId for continuity
  isolation?: Isolation;       // default {type:"None"} in runner
  verifyCommands?: string[];   // the ONLY verify commands the agent may run (#5)
  gate?: string;               // postcondition the TOOL runs after the agent (#7)
  allowPartialCommit?: boolean;// suppress the incomplete-commit concern (#1/#6)
  waitMs?: number;             // block boundary before auto-detach; clamp [1000,600000]
  background?: boolean;        // return {RUNNING, jobId} immediately
}

interface Usage { inputTokens; outputTokens; cacheReadTokens; cacheWriteTokens; } // all number

type RunStatus = "DONE" | "DONE_WITH_CONCERNS" | "BLOCKED" | "NEEDS_CONTEXT" | "ERROR";

interface ChangeSet {            // tool-computed git delta (#6), NOT the agent's self-report
  headBefore: string | null;
  headAfter: string | null;
  newCommits: string[];          // rev-list headBefore..HEAD
  filesChanged: string[];        // diff --name-only headBefore
  diffstat: string;              // diff --stat headBefore
  uncommittedFiles: string[];    // status --porcelain
  dirtyAfter: boolean;
}

interface GateResult { command: string; exitCode: number; passed: boolean; outputTail: string; }

interface RunOutput {
  status: RunStatus;
  text: string;                  // the agent's final result text
  sessionId: string | null;      // for resume
  backend: string;               // "cursor"
  model: string;                 // resolved model id
  usage: Usage | null;
  costUsd: number | null;
  costEstimated: boolean;        // always true (CLI emits no cost field)
  durationMs: number | null;
  stderrTail?: string;           // last ~2KB, present only on failure (#3)
  changeSet?: ChangeSet;         // present when cwd is a git repo (#6)
  gateResult?: GateResult;       // present when a gate ran (#7)
  jobId?: string;                // present when this is a detached job's terminal result (#4)
  concerns?: string[];           // human-readable advisories (e.g. incomplete commit)
}

// Raw shape emitted by the cursor-agent `result` event (verified 2026-06-09):
interface RawCursorJson {
  type?; subtype?; is_error?: boolean; duration_ms?; duration_api_ms?;
  result?: string; session_id?: string; request_id?: string; usage?: Usage;
}

interface ResolvedModel { backend: string; model: string; }
type TierMap  = Record<string, ResolvedModel>;
type PriceMap = Record<string, { input; output; cacheRead; cacheWrite }>; // per-million tokens
```

---

## 4. MCP tool surface

Six tools. All results are returned as a single MCP text-content block containing
`JSON.stringify(value, null, 2)` of the structured object below.

### 4.1 `cursor_run`
Input schema = `RunInput` (section 3); only `prompt` is required.

Behavior (delegates to `runDelegation` → registry `dispatch`):
1. Resolve model from `{tier, model}`.
2. Map `capability` (+`allowUnsandboxed`) to flags.
3. If a write capability → **verify deny-list** (throws/fails closed if missing).
4. Map `isolation` to `{flags, cwd}`.
5. Compose the prompt (preamble + verify-scope + prompt, then strip NULs).
6. Capture `HEAD` of `cwd` (for the change-set).
7. Build a `JobSpec` and dispatch under the deadline-race.

Return is one of:
- `RunOutput` — the task finished within the deadline (`waitMs` or default 60s).
- `{ status: "RUNNING", jobId, busyPath? }` — detached; poll/wait on it. `busyPath` is set for a
  write so the caller knows which tree is locked.
- `{ status: "BUSY", jobId, busyPath }` — refused: a write on a path with an in-flight write job.

### 4.2 `cursor_poll`  — input `{ jobId }`
Sub-second, non-blocking. Returns a `PollResult`:
- `{ status: "RUNNING", progress: { lastTool, tokensSoFar, elapsedMs, lastAssistant, filesTouchedSoFar, phase } }`
- `{ status: <terminal>, result: RunOutput }` for a finished job
- `{ status: "NOT_FOUND" }` if the id is unknown (or evicted from the completed-LRU).

### 4.3 `cursor_cancel` — input `{ jobId }`
SIGTERM the child, mark the job `CANCELLED`, await completion, return the resulting `PollResult`.
Cancelling an unknown/terminal job just returns its poll result.

### 4.4 `cursor_wait` — input `{ jobId, timeoutMs? }`
Long-poll: block until the job reaches a terminal status **or** `timeoutMs` elapses (default
120000, clamp `[1000, 600000]`). On timeout returns the `RUNNING` poll snapshot. Works on a `BUSY`
jobId too (waits for the lock holder). Replaces a poll/sleep loop.

### 4.5 `cursor_wait_any` — input `{ jobIds[], timeoutMs? }`
Block until the **first** listed job reaches a terminal status (or timeout). Returns
`{ jobs: Record<jobId, PollResult>, firstDone?: jobId }`. `NOT_FOUND` ids are ignored.

### 4.6 `cursor_wait_all` — input `{ jobIds[], timeoutMs? }`
Block until **all** listed (known) jobs are terminal (or timeout). Returns
`{ jobs: Record<jobId, PollResult>, allDone: boolean }`. Empty input → `{ jobs:{}, allDone:true }`.

> The wait/poll/cancel quartet is the public face of the async job model (#4); fan-in primitives
> (`wait_any`/`wait_all`) pair with `background:true` for parallel dispatch.

---

## 5. The `cursor-agent` CLI contract

This is the single external dependency the whole system is built around. Verified live against
`cursor-agent >= 2026.06`.

### 5.1 argv construction (pure)
```
cursor-agent --print --output-format stream-json --trust [--approve-mcps]
             --model <model>
             <capabilityFlags...>
             <isolationFlags...>
             [--resume <sessionId>]
             <prompt>
```
- `--print` = headless, non-interactive.
- `--output-format stream-json` = NDJSON event stream (NOT single-blob `json`) so progress can be
  surfaced incrementally.
- `--trust` always appended (trust the workspace).
- `--approve-mcps` appended by default in headless runs (auto-approve MCP servers).
- The **prompt is the last positional arg** and the only untrusted argv entry.
- Never emits `worker` (cloud) or a bare `--yolo`.

### 5.2 capability → flags
| capability | flags | notes |
|---|---|---|
| `ask` | `--mode ask` | read-only |
| `plan` | `--mode plan` | read-only |
| `write` | `--sandbox enabled --force` | no `--mode`: the default mode-less `--print` agent has write+shell; `--mode` now only accepts read-only plan/ask |
| `write-unsandboxed` *(with `allowUnsandboxed:true`)* | `--sandbox disabled --force` | |
| `write-unsandboxed` *(without the flag)* | `--sandbox enabled --force` | **downgraded** to `write`; report `downgraded:true` |

`--force` = "allow commands unless explicitly denied"; it is what makes write agents
non-interactive, and is why the deny-list (section 7) must be present.

### 5.3 isolation → {flags, cwd}
| isolation | flags | cwd |
|---|---|---|
| `{type:"None"}` | *(none)* | server cwd |
| `{type:"CallerProvided", path}` | `--workspace <path>` | `<path>` |
| `{type:"BackendProvided", name?, base?}` | `--worktree [name] [--worktree-base <base>]` | server cwd |

Only `CallerProvided` participates in the write-path lock (it names a shared working tree).

### 5.4 stream-json events (the NDJSON the parser must understand)
One JSON object per line. Relevant shapes:
```jsonc
// built-in tool call → lastTool = "shell" / "edit" / ... (key sans "ToolCall" suffix)
{"type":"tool_call","subtype":"started","tool_call":{"shellToolCall":{...}}}
// edit tool → also records the touched path
{"type":"tool_call","subtype":"started","tool_call":{"editToolCall":{"args":{"path":"..."}}}}
// MCP tool call → lastTool = its toolName (fallback "mcp")
{"type":"tool_call","subtype":"started","tool_call":{"mcpToolCall":{"toolName":"x"}}}
// assistant text → lastAssistant (concatenate text blocks; truncate to 200 chars)
{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
// thinking delta → phase (the "completed" subtype carries no text)
{"type":"thinking","subtype":"delta","text":"..."}
// TERMINAL: only this event carries the final result + usage + session_id + duration_ms
{"type":"result","subtype":"success","is_error":false,"result":"...","usage":{...},"session_id":"...","duration_ms":1234}
```
Parser rules:
- Ignore blank / non-JSON lines and events with no `type`.
- **Any** event carrying a `usage.outputTokens` number updates `tokensSoFar` (so the live token
  count generally only populates at the terminal `result`).
- Only `tool_call` with `subtype:"started"` updates `lastTool`/`filesTouched`.
- Buffer stdout and split on `\n`; on process close, flush a trailing line that arrived without a
  terminating newline (the `result` line often does).

---

## 6. The async job model (the architectural core, #4)

A single in-memory registry inside the long-lived stdio server owns every spawned child.

### 6.1 State
```
active:    Map<jobId, Job>           // in-flight
completed: Map<jobId, Job>           // terminal, LRU-capped at 100 (oldest evicted)
pathLock:  Map<path, jobId>          // CallerProvided write path → in-flight write job (#8)
```
`Job` holds: id, status, spec, child handle, startedAt, live progress (lastTool, tokensSoFar,
lastAssistant, filesTouched, phase), lastEventAt, terminationReason (`CANCELLED|STALLED|null`),
idleCancel timer handle, a `completion` promise, terminalOutput, and a `Set<ProgressSink>`.

`JobStatus = RUNNING | DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT | ERROR | CANCELLED | STALLED`.
`PollStatus = JobStatus | NOT_FOUND`.

### 6.2 dispatch — the deadline race
1. **#8 guard:** if `isWrite && path && pathLock.has(path)` → return `{BUSY, jobId, busyPath}` (no
   queue).
2. Generate a UUID jobId; `backend.run(spec)` spawns the child and returns `{child, events, done}`.
3. Register the job in `active`; if write+path, set `pathLock`.
4. Wire the caller's `ProgressSink` (if any) into `job.sinks`. On each `progress` event: update live
   fields, bump `lastEventAt`, **re-arm the idle watchdog**, and fan out a `ProgressUpdate` to every
   sink. `stderr` events also re-arm the watchdog.
5. `job.completion = done.then(finalizeJob, finalizeJobError)`.
6. **If `background:true`** → drop the sink and return `{RUNNING, jobId, busyPath?}` immediately.
7. Otherwise race three promises: `completion` vs a **deadline timer** vs the optional abort signal.
   - deadline = `clamp(waitMs)` if provided, else the configured `deadlineMs` (default 60000).
   - Winner `done` → return the terminal `RunOutput`. Else → return `{RUNNING, jobId, busyPath?}`.
   - **The deadline timer never kills the child** — it only decides sync-return vs detach.

### 6.3 Reaping (there is no fixed total-runtime cap)
Three and only three ways a job dies:
- **cancel** — SIGTERM, mark `CANCELLED`, await completion.
- **idle watchdog** — if no stream/stderr event arrives for `idleMs` (default 180000; `null`
  disables), SIGTERM and mark `STALLED`. Reaps genuinely-hung agents without an attentive caller and
  frees the path lock. Never kills a slow-but-working agent (events keep flowing).
- **shutdown** — on SIGTERM/SIGINT/exit, `killAll()` SIGTERMs every active child so ending the host
  session never leaves reparented orphans mutating the repo.

### 6.4 finalize
On child close the backend resolves `{ raw, cleanExit, stderr }`. `finalizeJob`:
1. `computeCost(raw.usage)` via the price map.
2. `finalizeRun(result, ctx)` (section 8) → terminal `RunOutput`.
3. `job.status = terminationReason ?? out.status` (so a cancelled/stalled job keeps that label),
   store `terminalOutput`, **retire** (move to `completed`, release path lock, LRU-evict).

`finalizeJobError` produces a synthetic `ERROR` RunOutput (or keeps `CANCELLED`/`STALLED`).

### 6.5 wait / waitAny / waitAll
Long-poll helpers. Each registers the sink (waitAny/waitAll tag updates with the 6-char jobId
prefix), races `completion`(s) vs a timeout vs abort, then returns poll snapshot(s). Default timeout
120000, clamp `[1000, 600000]`. `waitAll`'s `allDone` recomputes from poll state, treating
`NOT_FOUND` as "not blocking".

> **Accepted trade-offs:** all jobs are lost on server restart; terminal jobs survive only until
> LRU eviction (100). No persistence, no queue.

---

## 7. Configuration & safety surface

### 7.1 Files
| file | role |
|---|---|
| `config/tier-map.json` (bundled) | default `Tier → ResolvedModel` |
| `config/price-map.json` (bundled) | default per-million-token prices |
| `~/.config/cursor-delegate/host-profile.json` (per machine) | overrides + policy; path overridable via `$CURSOR_DELEGATE_HOST_PROFILE` |
| `~/.cursor/cli-config.json` (Cursor's own) | `permissions.deny` — the deny-list write calls are checked against |

`loadConfig` reads the two defaults and the host profile, then merges: `tierMap =
{...default, ...profile.tierOverrides}` (same for prices). A missing file (`ENOENT`) is treated as
empty/`null`, not an error.

### 7.2 Host profile shape
```jsonc
{
  "tierOverrides":  {},          // TierMap merged over the bundled default
  "priceOverrides": {},          // PriceMap merged over the bundled default
  "requiredDeny":   [],          // patterns that MUST be in cli-config deny before any write (#fail-closed)
  "promptPreamble": "...",       // standing instructions prepended to EVERY prompt (#2)
  "verifyCommands": [],          // default "only these verify commands" scope (#5)
  "gate":           "",          // default postcondition command the tool runs (#7)
  "deadlineMs":     60000,       // sync-vs-detach boundary (#4)
  "idleMs":         180000       // idle watchdog window; null disables (#4)
}
```

### 7.3 Fail-closed deny-list (`safety.ts`)
Before any `write`/`write-unsandboxed` call: read `~/.cursor/cli-config.json`; if every pattern in
`requiredDeny` is **not** present in `permissions.deny`, throw `DenyListError` and refuse to run.
`requiredDeny: []` → always passes. Read-only (`ask`/`plan`) calls skip the check. Rationale:
`--force` makes write agents non-interactive, so the only guard against a destructive command is
the deny-list — verify it exists before trusting it. The host profile and the cli-config deny-list
are **per-machine** and must not be copied between hosts (the example patterns target one GPU host).

### 7.4 Defaults shipped
```jsonc
// tier-map.json
{ "cheap-bulk":        {"backend":"cursor","model":"composer-2.5"},
  "standard":          {"backend":"cursor","model":"gemini-3.5-flash"},
  "coding-specialist": {"backend":"cursor","model":"composer-2.5"},
  "diversity":         {"backend":"cursor","model":"gpt-5.5-medium"} }
// price-map.json (USD per 1M tokens: input, output, cacheRead, cacheWrite)
{ "composer-2.5":     {"input":0.5,"output":2.5,"cacheRead":0.2,"cacheWrite":0},
  "gemini-3.5-flash": {"input":1.5,"output":9,  "cacheRead":0.15,"cacheWrite":0},
  "gpt-5.5-medium":   {"input":5,  "output":30, "cacheRead":0.5, "cacheWrite":0},
  "gpt-5.5":          {"input":5,  "output":30, "cacheRead":0.5, "cacheWrite":0} }
```

---

## 8. Pure-logic specifications (reimplement these exactly)

### 8.1 Model resolution (`tiers.ts`)
Order: **raw `model` override → named `tier` → default `cheap-bulk`**.
- A raw `model` returns `{backend:"cursor", model}` directly.
- A named tier looks up the merged tier-map; missing entry → throw.
- **Diversity contract:** if `tier === "diversity"` and the resolved/override model matches
  `/claude|opus|sonnet|haiku/i`, throw `DiversityClaudeError`. (Diversity must be a non-Claude
  second opinion.)

### 8.2 Prompt composition (`prompt.ts`)
Join, in order, with separator `"\n\n---\n\n"`: `[preamble?, verifyBlock(verifyCommands)?, prompt]`,
then strip all NUL bytes (`s.replace(/\0/g, "")` — `spawn` throws on a NUL in any argv entry; #1).
`verifyBlock` renders: *"These are the ONLY verification commands you may run: `c1`, `c2`. Do not
run workspace-wide builds (e.g. `cargo check --workspace`, full test suites) or any other build/test
command."* (#5). Per-call `verifyCommands` overrides the profile default; same for `gate`.

### 8.3 Status derivation (`output.ts`)
`text = raw.result ?? ""`. Precedence:
1. An explicit trailing `STATUS: <X>` line in `text` (regex `/STATUS:\s*([A-Z_]+)\s*$/m`, accepted
   only if `X ∈ RunStatus`).
2. Else `raw.is_error === false && cleanExit` → `DONE`.
3. Else `ERROR`.

Agents are conventionally instructed to end with `STATUS: DONE | BLOCKED | NEEDS_CONTEXT` etc.

### 8.4 Finalize pipeline (`finalize.ts`, #3/#6/#7)
Given `BackendResult` + context, build `RunOutput` then layer on:
1. base via `toRunOutput`; attach `jobId` if detached.
2. **#3** `stderrTail` = last 2048 bytes of stderr, but **only** if `!cleanExit || status==="ERROR"`
   and stderr is non-empty (keep it off the happy path).
3. **#7** if a `gate` is set: run it (section 8.5); a failed gate downgrades a clean `DONE` →
   `DONE_WITH_CONCERNS`.
4. **#6** compute `changeSet = gitDelta(cwd, headBefore)` (null if not a repo); attach if present.
5. **Incomplete-commit concern:** if `isWrite && newCommits>0 && uncommittedFiles>0 &&
   !allowPartialCommit` → push a human-readable concern and downgrade `DONE` →
   `DONE_WITH_CONCERNS` (HEAD may not build).

### 8.5 Gate (`gate.ts`, #7)
Run `command` via `/bin/sh -c` in `cwd`, `maxBuffer` 16 MiB. `passed = exitCode === 0`.
`outputTail` = last 2048 bytes of `stdout+stderr`. Never throws (resolves a `GateResult`).
*Distinct from `verifyCommands`*: the gate is the **tool's enforced postcondition**; `verifyCommands`
is the **agent's self-scope** injected into the prompt.

### 8.6 Git delta (`git.ts`, #6)
Best-effort (any git failure → `null`, never throws; uses `git -C <cwd> ...`, `maxBuffer` 16 MiB).
- `headAfter = rev-parse HEAD`; if null → not a repo → return null.
- If `headBefore`: `newCommits = rev-list headBefore..HEAD`; `filesChanged = diff --name-only
  headBefore`; `diffstat = diff --stat headBefore`.
- `uncommittedFiles = parse(status --porcelain)` (strip the 2-char XY + space prefix; handle `->`
  renames; unquote quoted paths). `dirtyAfter = uncommittedFiles.length > 0`.

### 8.7 Cost (`pricing.ts`)
Always best-effort, `costEstimated` always `true` (CLI emits no cost field). `null` if usage or a
price entry is missing. Else `Σ(tokens_k × price_k) / 1e6` over input/output/cacheRead/cacheWrite.
Alias: bare `gpt-5.5` → `gpt-5.5-medium` for price lookup.

### 8.8 cursor-agent binary resolution (`cursor-bin.ts`)
Order: explicit override → `$CURSOR_AGENT_BIN` → `which cursor-agent` → fallback
`~/.local/bin/cursor-agent`.

---

## 9. Live progress → MCP notifications (`progress.ts`)

`ProgressUpdate = { lastTool, tokensSoFar, elapsedMs, phase?, jobTag? }`.
`formatProgress(u)` → `"[jobTag ]<lastTool|thinking> · <tok> tok · <sec>s"`.

`progressSinkFrom(extra)` bridges the registry's `ProgressSink` to the MCP transport:
- Returns `undefined` if the client supplied no `extra._meta.progressToken` (no progress wanted).
- Otherwise returns a sink that calls `extra.sendNotification({ method:"notifications/progress",
  params:{ progressToken, progress: ++seq, message: formatProgress(u) } })`.
- **Throttle:** emit immediately when `lastTool` changes; otherwise drop updates that arrive `<1000ms`
  after the last emission. (Avoids flooding the client on token-only updates.)

In `index.ts`, the `progressSink` is derived per call from the MCP request's `extra` and passed to
`dispatch`/`wait*`; `extra.signal` is forwarded as the abort signal.

---

## 10. Server wiring & lifecycle (`index.ts`)

- Build paths relative to the module (`dist/..` or `src/..`) to locate bundled `config/*.json`.
- `loadConfig(...)`; read `~/.cursor/cli-config.json` (tolerate missing — fail-closed handled
  per-call).
- `makeJobRegistry({ backend: makeCursorAdapter(), deadlineMs: cfg.deadlineMs ?? 60000,
  idleMs: cfg.idleMs === undefined ? 180000 : cfg.idleMs })`.
- `deps = { config, registry, cliConfig, serverCwd: process.cwd() }`.
- MCP `Server({name:"cursor-delegate", version:"0.1.0"}, {capabilities:{tools:{}}})`:
  - `ListTools` → the 6 tool descriptors (rich, model-facing descriptions; see section 4 and the
    reference `RUN_INPUT_SCHEMA`).
  - `CallTool` → validate required field per tool, derive the progress sink + abort signal from
    `extra`, route to the matching `handleCursor*`, wrap the result as a JSON text-content block.
- Shutdown: on `SIGTERM`/`SIGINT`/`exit` call `registry.killAll()`.
- Connect a `StdioServerTransport`. Guard `main()` so importing the module in tests doesn't start
  the server (`if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1])`).

---

## 11. The 8 dogfooded improvements (cross-reference)

These `#N` tags appear throughout the code and this spec:

| # | Improvement | Where |
|---|---|---|
| #1 | NUL-byte prompt sanitization; incomplete-commit concern (`allowPartialCommit`) | `prompt.ts`, `finalize.ts` |
| #2 | `promptPreamble` standing instructions | `config.ts`, `prompt.ts` |
| #3 | `stderrTail` on failure only | `finalize.ts` |
| #4 | Async job model: deadline-race, detach+jobId, idle watchdog, no hard cap, poll/wait/cancel | `job-registry.ts`, `index.ts` |
| #5 | `verifyCommands` hard-scope bound (agent self-scope) | `prompt.ts`, `config.ts` |
| #6 | Tool-computed git change-set (ground truth, not self-report) | `git.ts`, `finalize.ts` |
| #7 | Tool-run `gate` postcondition (tool-enforced) | `gate.ts`, `finalize.ts` |
| #8 | Same-path write serialization (BUSY, no queue) | `job-registry.ts` |

---

## 12. Agent catalog (convention layer, not code)

`config/agents/catalog.md` documents reusable "agents" as **convention tuples over `cursor_run`** —
the controller (the host agent) constructs each call; nothing here is Cursor-side config. The
governing principle: *never review an agent's output with the same model that produced it.* Example
tuples: **Verifier** (tier `diversity`, `ask`, adversarial-refute prompt), **Triager** (`cheap-bulk`,
`ask`), **Design-critic** (`diversity`, `plan`), **Codemod**/**Re-implementer**/**SP-implementer**
(`coding-specialist`/`cheap-bulk`, `write`, `CallerProvided`), **SP spec/quality reviewers**
(`standard` = a different engine than the implementer). Reproduce this as documentation, not logic.

---

## 13. Packaging & install

### 13.1 As a Claude Code plugin
```jsonc
// plugin/plugin.json
{ "name":"cursor-delegate", "version":"0.1.0", "description":"...", "mcpServers":".mcp.json" }
// plugin/.mcp.json
{ "mcpServers": { "cursor-delegate": {
    "type":"stdio", "command":"node", "args":["${CLAUDE_PLUGIN_ROOT}/../dist/index.js"] } } }
```

### 13.2 Build & register (`bin/setup.sh`, portable Linux/macOS)
1. Resolve **this machine's** `node` (`command -v node`) — never bake a path.
2. Warn if `cursor-agent` not on PATH (prerequisite: installed + `cursor-agent login`).
3. `npm install && npm run build` (`tsc` → `dist/`; pure JS, no native deps).
4. Scaffold a **minimal** `~/.config/cursor-delegate/host-profile.json` (`requiredDeny: []`) **only
   if absent**; never overwrite an existing profile or the cli-config deny-list.
5. Register at user scope: `claude mcp add <name> -s user -- "<node>" "<dist>/index.js"` (idempotent:
   remove-then-add). `DRY_RUN=1` previews without changes.

### 13.3 Manual register (no `claude` CLI)
`claude mcp add cursor-delegate -s user -- <node> <repo>/dist/index.js`, or add the stdio entry to
your MCP client's config directly.

### 13.4 package.json essentials
```jsonc
{ "type":"module",
  "bin": { "cursor-delegate-mcp":"dist/index.js" },
  "scripts": { "build":"tsc",
               "test":"node --import tsx --test \"tests/**/*.test.ts\"",
               "test:live":"node --import tsx --test \"tests/**/*.live.test.ts\"" },
  "dependencies": { "@modelcontextprotocol/sdk":"^1.0.0" },
  "devDependencies": { "tsx":"^4", "typescript":"^5.6", "@types/node":"^22" } }
```
`tsconfig`: `target ES2022`, `module/moduleResolution NodeNext`, `strict`, `rootDir src`,
`outDir dist`.

---

## 14. Prerequisites (per machine, not bundled)
1. `cursor-agent` CLI installed and logged in (`cursor-agent status`).
2. A host profile at `~/.config/cursor-delegate/host-profile.json`.
3. The host deny-list merged into `~/.cursor/cli-config.json` `permissions.deny` (required for any
   write capability).

---

## 15. Testing strategy

Every impurity is injected, so units test against fakes: a `spawnFn` that replays canned
stream-json lines, a fake `Clock` (`now()` + `setTimer`) to drive deadline/idle/timeout
deterministically, an injected `finalize`, and an in-memory config reader. There is one
`integration.live.test.ts` (real `cursor-agent`, opt-in via `npm run test:live`). Coverage mirrors
the module list: capability, isolation, safety, prompt, tiers, pricing, output, git, gate, stream,
config, job-registry, cursor adapter/bin, finalize, progress, plus an `index` smoke test.

Key invariants worth asserting when reproducing:
- write capability with a missing deny pattern **throws before spawn**;
- `diversity` + a Claude model **throws**;
- a job that exceeds the deadline returns `{RUNNING, jobId}` and is later pollable to terminal;
- a second write to a locked `CallerProvided` path returns `{BUSY}`;
- idle watchdog SIGTERMs and marks `STALLED`; cancel marks `CANCELLED`; both survive finalize;
- `stderrTail` present only on non-clean exit; `gate` failure downgrades `DONE`→`DONE_WITH_CONCERNS`;
- incomplete-commit (commits + dirty tree) downgrades unless `allowPartialCommit`.
```
