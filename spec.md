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

It exposes a generic delegation tool (`cursor_run`), job-control tools (`cursor_poll`,
`cursor_cancel`, `cursor_wait`, `cursor_wait_any`, `cursor_wait_all`), a needs-input resume tool
(`cursor_answer`), and a diagnostics tool (`doctor`). The value it adds over calling `cursor-agent`
directly:

- **Curated model allow-list** — a single bundled `config/models.json` lists callable model ids with
  labels, family tags, and hand-maintained `$/MTok` prices. A model is callable iff it is in the
  map; the MCP `model` enum is generated from that map at startup. Optional `requireNonClaude`
  hard-rejects when the resolved model's `family === "claude"` (no silent swap).
- **Capability modes** — `ask` / `plan` (read-only) vs `write` / `write-unsandboxed`, each mapping
  to a vetted, non-interactive `cursor-agent` flag set.
- **Sandboxing + a fail-closed deny-list** — write calls are refused unless the host's
  `cursor-agent` deny-list contains every required pattern.
- **An async job model** — fast tasks return synchronously; slow tasks detach and hand back a
  `jobId` you poll/wait on. Live progress streams to the client while a call blocks.
- **Needs-input round-trip** — delegates are instructed to end with `STATUS: NEEDS_CONTEXT` when
  they need an orchestrator answer; the run is parked with its `sessionId` + resume context, and
  `cursor_answer(jobId, answer)` resumes via `--resume`.
- **Ground-truth verification** — the tool itself computes the git change-set over the run, runs an
  optional postcondition "gate" command, and surfaces stderr on failure — rather than trusting the
  agent's self-report.
- **Same-path write serialization** — two concurrent writes to one working tree are refused, not
  interleaved.
- **Doctor diagnostics** — probe binary, login, and allow-list drift against the account model menu.

It is **local headless only**: it never spawns Cursor *cloud* (`worker`) runs and never passes a
bare `--yolo`.

---

## 2. Architecture & module decomposition

Pure functions for policy; one stateful module (the job registry) for process lifecycle. Every
impure boundary (spawn, git, gate, clock, config read, doctor CLI probes) is injected so it can be
faked in tests.

```
MCP client (Claude Code)
   │  stdio (JSON-RPC)
   ▼
index.ts ............ MCP server: tool list (buildTools), request routing, arg validation,
   │                  progress-sink wiring, shutdown handlers
   ▼
runner.ts ........... runDelegation(): pure pre-flight — resolve model, map capability,
   │                  verify deny-list, map isolation, compose prompt, capture HEAD,
   │                  build a JobSpec (incl. ResumeContext), hand to the registry
   │                  answerDelegation(): lookupAnswer → re-enter runDelegation with
   │                  --resume <sessionId> and answer as the prompt
   ▼
job-registry.ts ..... STATEFUL. spawn via backend, deadline-race, detach+jobId,
   │                  progress tracking, idle watchdog, write-path lock, cancel,
   │                  poll/wait/waitAny/waitAll, lookupAnswer, shutdown killAll
   ├── backends/cursor.ts ... spawn cursor-agent, parse stream-json (NDJSON) lines,
   │      backends/types.ts    emit "progress"/"stderr" events, resolve a BackendResult
   │   stream.ts ............. incremental stream-json parser (lastTool, tokens, files, phase)
   │   cursor-bin.ts ......... resolve the cursor-agent binary path
   └── finalize.ts ......... assemble terminal RunOutput:
          output.ts ......... base status/usage/text from the raw result blob
          gate.ts ........... run the postcondition command (#7)
          git.ts ............ compute the real change-set (#6)

Policy helpers (pure), consumed by runner.ts / index.ts:
   models.ts ....... model id → ResolvedModel (+ requireNonClaude hard reject)
   capability.ts ... capability → cursor-agent flags (+ unsandboxed downgrade)
   isolation.ts .... isolation → {flags, cwd}
   safety.ts ....... fail-closed deny-list verification
   prompt.ts ....... preamble + verify-scope + statusBlock + NUL sanitization
   pricing.ts ...... usage × priceMap → best-effort USD
   config.ts ....... load models.json + host profile; derive priceMap
   tool-schemas.ts . buildRecommendedModelsBlurb / buildRunInputSchema / buildTools
   doctor.ts ....... parseAbout / parseModelsList / probes / runDoctor
   progress.ts ..... ProgressSink + MCP notifications/progress bridge
   validate.ts ..... MCP cursor_run args → typed RunInput
   types.ts ........ all domain types
```

**Dependency rule:** `runner` and the policy helpers are pure and synchronous-ish (config is async
I/O only). All process/timer state lives in `job-registry`. `index` is the only module that touches
the MCP transport. `doctor` is invoked from `index` and does not touch the registry.

---

## 3. Domain types

```ts
type Capability = "ask" | "plan" | "write" | "write-unsandboxed";

type Isolation =
  | { type: "None" }
  | { type: "CallerProvided"; path: string }
  | { type: "BackendProvided"; name?: string; base?: string };

interface RunInput {
  prompt: string;              // required
  model?: string;              // curated allow-list id; omit → config.default
  requireNonClaude?: boolean;  // hard-reject if resolved family === "claude"
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
  sessionId: string | null;      // for resume / cursor_answer
  backend: string;               // "cursor"
  model: string;                 // resolved model id
  usage: Usage | null;
  costUsd: number | null;
  costEstimated: boolean;        // always true (CLI emits no cost field)
  durationMs: number | null;
  stderrTail?: string;           // last ~2KB, present only on failure (#3)
  changeSet?: ChangeSet;         // present when cwd is a git repo (#6)
  gateResult?: GateResult;       // present when a gate ran (#7)
  jobId?: string;                // always attached by finalize for registry jobs
  concerns?: string[];           // human-readable advisories (e.g. incomplete commit)
  downgraded?: boolean;          // write-unsandboxed silently downgraded to write
}

// Raw shape emitted by the cursor-agent `result` event (verified 2026-06-09):
interface RawCursorJson {
  type?; subtype?; is_error?: boolean; duration_ms?; duration_api_ms?;
  result?: string; session_id?: string; request_id?: string; usage?: Usage;
}

interface Price {
  input: number; output: number; cacheRead: number; cacheWrite: number; // $/MTok
}
type PriceMap = Record<string, Price>;

interface ModelEntry { label: string; family: string; price: Price; }

interface ResolvedModel { model: string; family: string; price: Price; }

/** Fields needed to resume a parked NEEDS_CONTEXT job via cursor_answer. */
interface ResumeContext {
  model: string;
  requireNonClaude?: boolean;
  capability: Capability;
  allowUnsandboxed: boolean;
  isolation: Isolation;
  verifyCommands?: string[];
  gate: string;                  // resolved gate string ("" = no gate)
  allowPartialCommit: boolean;
}

interface HostProfile {
  default?: string;              // override bundled default model id
  models?: Record<string, ModelEntry>; // merge over bundled models
  requiredDeny?: string[];
  promptPreamble?: string;
  verifyCommands?: string[];
  gate?: string;
  deadlineMs?: number;
  idleMs?: number | null;
}

interface Config {
  default: string;
  models: Record<string, ModelEntry>;
  priceMap: PriceMap;            // derived from models at load time
  profile: HostProfile;
}

/** Doctor report (see section 4.8 / 8.9). */
interface DoctorReport {
  ok: boolean;                   // failures.length === 0
  plugin: { version: string };
  agent: { found: boolean; path: string | null; version: string | null; error?: string };
  account: {
    loggedIn: boolean;
    email: string | null;
    subscription: string | null; // account plan from `cursor-agent about`
    currentModel: string | null;
    error?: string;
  };
  modelMenu: {
    configuredIds: string[];
    accountIds: string[] | null;
    missingFromAccount: string[];
    pricesCheckable: false;
    note: string;
    error?: string;
  };
  warnings: string[];
  failures: string[];
}
```

---

## 4. MCP tool surface

Eight tools. All results are returned as a single MCP text-content block containing
`JSON.stringify(value, null, 2)` of the structured object below. Tool descriptors are built at
startup via `buildTools(config)` — the `cursor_run` `model` enum and recommended-models blurb are
dynamic from the allow-list.

### 4.1 `cursor_run`
Input schema = `RunInput` (section 3); only `prompt` is required. The `model` property's JSON Schema
`enum` is the sorted allow-list ids; its description names the default. A recommended-models blurb
(`id — label — $in/$out`, sorted by id) is rendered into the tool description.

Behavior (delegates to `runDelegation` → registry `dispatch`):
1. Resolve model from `{model, requireNonClaude}` against the curated map.
2. Map `capability` (+`allowUnsandboxed`) to flags.
3. If the capability is `forced` (all of them — every cap carries `--force`) → **verify deny-list**
   (throws/fails closed if missing).
4. Map `isolation` to `{flags, cwd}`.
5. Compose the prompt (preamble + verify-scope + statusBlock + prompt, then strip NULs).
6. Capture `HEAD` of `cwd` (for the change-set; for `BackendProvided`, capture from server cwd /
   `base` before the worktree exists).
7. Build a `JobSpec` (including `ResumeContext`) and dispatch under the deadline-race.

Return is one of:
- `RunOutput` — the task finished within the deadline (`waitMs` or default 60s). Always carries
  `jobId` (so a foreground `NEEDS_CONTEXT` is answerable via `cursor_answer` without a detach).
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

### 4.7 `cursor_answer` — input `{ jobId, answer }` (both required)
Resume a parked `NEEDS_CONTEXT` job:
1. `registry.lookupAnswer(jobId)` → `{ sessionId, resumeContext }` or an error.
2. Unknown/expired `jobId` → return `{ status: "NOT_FOUND" }`.
3. Job present but not awaiting input (`status !== "NEEDS_CONTEXT"` or missing `sessionId`) →
   throw / reject with `"job is not awaiting an answer"`.
4. Otherwise call `runDelegation` with `prompt: answer`, `session: sessionId`, and the stored
   `ResumeContext` fields (`model`, `requireNonClaude`, `capability`, `allowUnsandboxed`,
   `isolation`, `verifyCommands`, `gate`, `allowPartialCommit`). Returns the same shape as
   `cursor_run` (terminal, `NEEDS_CONTEXT` again, or `RUNNING`/`jobId`).

### 4.8 `doctor` — input `{ deep?: boolean }` (all optional; `deep` reserved and currently ignored)
Runs `runDoctor({ config, deep })` and returns a `DoctorReport`. See section 8.9 for probe rules.
Missing configured ids and model-menu probe errors are **warnings**; missing binary / not-logged-in
are **failures**; `ok === (failures.length === 0)`. Prices are not CLI-checkable.

> The wait/poll/cancel quartet is the public face of the async job model (#4); fan-in primitives
> (`wait_any`/`wait_all`) pair with `background:true` for parallel dispatch. `cursor_answer` is the
> single resume path for needs-input (foreground or backgrounded).

---

## 5. The `cursor-agent` CLI contract

This is the single external dependency the whole system is built around. Verified live against
`cursor-agent >= 2026.06`.

### 5.1 argv construction (pure)
```
cursor-agent --print --output-format stream-json --trust --approve-mcps
             --model <model>
             <capabilityFlags...>
             <isolationFlags...>
             [--resume <sessionId>]
             -- <prompt>
```
- `--print` = headless, non-interactive.
- `--output-format stream-json` = NDJSON event stream (NOT single-blob `json`) so progress can be
  surfaced incrementally.
- `--trust` always appended (trust the workspace).
- `--approve-mcps` appended by default in headless runs (auto-approve MCP servers).
- `--` then the **prompt as the last positional arg** — the only untrusted argv entry.
- Never emits `worker` (cloud) or a bare `--yolo`.

### 5.2 capability → flags
| capability | flags | notes |
|---|---|---|
| `ask` | `--mode ask --force` | read-only (no edits) but **runs shell** — e.g. `git show` for branch-only content |
| `plan` | `--mode plan --force` | read-only (no edits) but **runs shell** |
| `write` | `--sandbox enabled --force` | no `--mode`: the default mode-less `--print` agent has write+shell; `--mode` now only accepts read-only plan/ask |
| `write-unsandboxed` *(with `allowUnsandboxed:true`)* | `--sandbox disabled --force` | |
| `write-unsandboxed` *(without the flag)* | `--sandbox enabled --force` | **downgraded** to `write`; report `downgraded:true` |

`--force` = "allow commands unless explicitly denied". **Every** capability carries it:
a headless (`--print`) run has no interactive approval channel, so without `--force` an
`ask`/`plan` agent blocks forever the instant it tries to run a shell command (verified:
`--mode plan` alone → timeout / 0 bytes; `--force --mode plan` / `--force --mode ask` →
real `git show` output, exit 0). `--mode ask`/`plan` still bar edits even with `--force`,
so read-only + `--force` = "run any read-only command unprompted". Because `--force` makes
command execution non-interactive for read-only caps too, the deny-list (section 7) gates
**all** capabilities, not just writes — `CapabilityResult.forced` (true for every cap)
drives the gate, distinct from `isWrite` (which still drives the write-path lock and
change-set finalize).

### 5.3 isolation → {flags, cwd}
| isolation | flags | cwd |
|---|---|---|
| `{type:"None"}` | *(none)* | server cwd |
| `{type:"CallerProvided", path}` | `--workspace <path>` | `<path>` |
| `{type:"BackendProvided", name?, base?}` | `--worktree [name] [--worktree-base <base>]` | server cwd |

Only `CallerProvided` participates in the write-path lock (it names a shared working tree).
For `BackendProvided`, `headBefore` is captured from the server repo (optionally at `base`) before
the worktree exists; finalize later resolves the worktree path for change-set/gate.

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
- `phase` tracks current activity: `tool_call:started` → `"running_tool"`, `assistant` text →
  `"responding"`, `thinking:delta` → `"thinking"`. The registry's idle watchdog uses `"running_tool"`
  to switch from `idleMs` to the wider `toolIdleMs` — a running build/test going silent for minutes
  is not a hang, but silence with no tool in flight (waiting on the model itself) still is.
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
`Job` holds: id, status, spec (incl. `resumeContext`), child handle, startedAt, live progress
(lastTool, tokensSoFar, lastAssistant, filesTouched, phase), lastEventAt, terminationReason
(`CANCELLED|STALLED|null`), idleCancel timer handle, a `completion` promise, terminalOutput, and a
`Set<ProgressSink>`.

`JobStatus = RUNNING | DONE | DONE_WITH_CONCERNS | BLOCKED | NEEDS_CONTEXT | ERROR | CANCELLED | STALLED`.
`PollStatus = JobStatus | NOT_FOUND`.

`AnswerLookup` (for `cursor_answer`):
- `{ ok: true, sessionId, resumeContext }` when `status === "NEEDS_CONTEXT"` and
  `terminalOutput.sessionId` is non-null
- `{ ok: false, error: "NOT_FOUND" }` when the id is unknown/evicted
- `{ ok: false, error: "NOT_AWAITING", status }` otherwise

### 6.2 dispatch — the deadline race
1. **#8 guard:** if `isWrite && path && pathLock.has(path)` → return `{BUSY, jobId, busyPath}` (no
   queue).
2. Generate a UUID jobId; `backend.run(spec)` spawns the child and returns `{child, events, done}`.
3. Register the job in `active`; if write+path, set `pathLock`.
4. Wire the caller's `ProgressSink` (if any) into `job.sinks`. On each `progress` event: update live
   fields (incl. `phase`), bump `lastEventAt`, **re-arm the idle watchdog**, and fan out a
   `ProgressUpdate` to every sink. `stderr` and raw stdout `activity` events also re-arm the watchdog
   without waiting for a full parsed line.
5. `job.completion = done.then(finalizeJob, finalizeJobError)`.
6. **If `background:true`** → drop the sink and return `{RUNNING, jobId, busyPath?}` immediately.
7. Otherwise race three promises: `completion` vs a **deadline timer** vs the optional abort signal.
   - deadline = `clamp(waitMs)` if provided, else the configured `deadlineMs` (default 60000).
   - Winner `done` → return the terminal `RunOutput` (includes `jobId`). Else → return
     `{RUNNING, jobId, busyPath?}`.
   - **The deadline timer never kills the child** — it only decides sync-return vs detach.

### 6.3 Reaping (there is no fixed total-runtime cap)
Three and only three ways a job dies:
- **cancel** — SIGTERM, mark `CANCELLED`, await completion.
- **idle watchdog** — tiered by `phase` (#9): if no stream/stderr/stdout-activity arrives for
  `idleMs` (default 300000; `null` disables) while no tool is in flight, or for the wider
  `toolIdleMs` (default 1800000; `null` disables) while `phase === "running_tool"`, SIGTERM and mark
  `STALLED`. Reaps genuinely-hung agents without an attentive caller and frees the path lock. Never
  kills a slow-but-working agent (events, or even raw bytes, keep flowing) — and no longer kills an
  agent that is legitimately mid-build/mid-test with nothing to say in the meantime.
- **shutdown** — on SIGTERM/SIGINT/exit, `killAll()` SIGTERMs every active child so ending the host
  session never leaves reparented orphans mutating the repo.

### 6.4 finalize
On child close the backend resolves `{ raw, cleanExit, stderr }`. `finalizeJob`:
1. If `terminationReason` is set (cancel/stall): build a base output via `baseOutput` (skip gate +
   git), attach `jobId`, retire.
2. Else `finalizeRun(result, ctx)` (section 8) → terminal `RunOutput` (always with `jobId`).
3. `job.status = terminationReason ?? out.status` (so a cancelled/stalled job keeps that label;
   `NEEDS_CONTEXT` is preserved for parking), store `terminalOutput`, **retire** (move to
   `completed`, release path lock, LRU-evict).

`finalizeJobError` produces a synthetic `ERROR` RunOutput (or keeps `CANCELLED`/`STALLED`).

### 6.5 Needs-input parking
Any run whose terminal status is `NEEDS_CONTEXT` is retained in `completed` like any other terminal
job, carrying `sessionId` on `terminalOutput` and the original `ResumeContext` on `spec`. That is
what makes `lookupAnswer` / `cursor_answer` work for both foreground and backgrounded runs. Eviction
from the completed-LRU (cap 100) makes the jobId expire → `NOT_FOUND`.

### 6.6 wait / waitAny / waitAll
Long-poll helpers. Each registers the sink (waitAny/waitAll tag updates with the 6-char jobId
prefix), races `completion`(s) vs a timeout vs abort, then returns poll snapshot(s). Default timeout
120000, clamp `[1000, 600000]`. `waitAll`'s `allDone` recomputes from poll state, treating
`NOT_FOUND` as "not blocking".

> **Accepted trade-offs:** all jobs are lost on server restart; terminal jobs survive only until
> LRU eviction (100). No persistence, no queue. Needs-input detection leans on the model emitting
> the trailing `STATUS:` line; the orchestrator's review of the result is the backstop.

---

## 7. Configuration & safety surface

### 7.1 Files
| file | role |
|---|---|
| `config/models.json` (bundled) | curated allow-list: `{ default, models: { <id>: { label, family, price } } }` |
| `~/.config/cursor-delegate/host-profile.json` (per machine) | overrides + policy; path overridable via `$CURSOR_DELEGATE_HOST_PROFILE` |
| `~/.cursor/cli-config.json` (Cursor's own) | `permissions.deny` — the deny-list write calls are checked against |

`loadConfig({ modelsPath })` reads the bundled models file and the host profile, then merges:
- `default = profile.default ?? file.default`
- `models = { ...file.models, ...profile.models }`
- `priceMap` derived from the merged `models` (each entry's `price`) for `computeCost`
- Throws if the models file is missing/invalid, or if the merged `default` id is absent from
  `models`. A missing host profile (`ENOENT`) is treated as empty, not an error.

### 7.2 Host profile shape
```jsonc
{
  "default":        "composer-2.5", // optional override of bundled default
  "models":         {},             // optional ModelEntry map merged over bundled models
  "requiredDeny":   [],             // patterns that MUST be in cli-config deny before any write (#fail-closed)
  "promptPreamble": "...",          // standing instructions prepended to EVERY prompt (#2)
  "verifyCommands": [],             // default "only these verify commands" scope (#5)
  "gate":           "",             // default postcondition command the tool runs (#7)
  "deadlineMs":     60000,          // sync-vs-detach boundary (#4)
  "idleMs":         180000          // idle watchdog window; null disables (#4)
}
```

### 7.3 Fail-closed deny-list (`safety.ts`)
Before **any** delegation (`ask`/`plan`/`write`/`write-unsandboxed`): read
`~/.cursor/cli-config.json`; if every pattern in `requiredDeny` is **not** present in
`permissions.deny`, throw `DenyListError` and refuse to run. `requiredDeny: []` → always passes.
The gate fires whenever `CapabilityResult.forced` is true, which is every capability. Rationale:
`--force` makes command execution non-interactive for read-only caps too — a `--force --mode plan`
agent can't edit files but can run any non-denied shell command (`curl`, `git push`, …) unprompted,
so the deny-list is the only guard; verify it exists before trusting it. (Read-only caps used to
skip this check when they lacked `--force` and thus couldn't run commands at all.) The host profile
and the cli-config deny-list are **per-machine** and must not be copied between hosts (the example
patterns target one GPU host).

### 7.4 Defaults shipped (`config/models.json`)
```jsonc
{
  "default": "composer-2.5",
  "models": {
    "composer-2.5":     { "label": "Composer 2.5",    "family": "composer", "price": { "input": 0.5, "output": 2.5, "cacheRead": 0.2,  "cacheWrite": 0 } },
    "grok-4.5-xhigh":   { "label": "Grok 4.5",         "family": "grok",     "price": { "input": 2,   "output": 6,   "cacheRead": 0.5,  "cacheWrite": 0 } },
    "gemini-3.5-flash": { "label": "Gemini 3.5 Flash", "family": "gemini",   "price": { "input": 1.5, "output": 9,   "cacheRead": 0.15, "cacheWrite": 0 } },
    "gpt-5.5-high":     { "label": "GPT-5.5 1M High",  "family": "gpt",      "price": { "input": 5,   "output": 30,  "cacheRead": 0.5,  "cacheWrite": 0 } }
  }
}
```
Prices are `$/MTok`, hand-maintained (the CLI exposes no pricing). `family` is a free string; the
enforced rule only distinguishes `"claude"` from the rest. No Claude entry is seeded — nothing
prevents adding one later. A model is callable **iff** it is in the merged map (no raw-passthrough
escape hatch).

---

## 8. Pure-logic specifications (reimplement these exactly)

### 8.1 Model resolution (`models.ts`)
`resolveModel({ model, requireNonClaude }, { default, models })`:
1. `model = model ?? config.default`.
2. Look up `config.models[model]`; if absent → throw `ModelNotAllowedError`.
3. If `requireNonClaude` and `entry.family === "claude"` → throw `NonClaudeViolationError`
   (hard reject — covers both an explicit Claude model and a Claude default; no silent swap).
4. Return `{ model, family: entry.family, price: entry.price }`.

### 8.2 Prompt composition (`prompt.ts`)
Join, in order, with separator `"\n\n---\n\n"`:
`[preamble?, verifyBlock(verifyCommands)?, statusBlock(), prompt]`, then strip all NUL bytes
(`s.replace(/\0/g, "")` — `spawn` throws on a NUL in any argv entry; #1).

`verifyBlock` renders: *"These are the ONLY verification commands you may run: `c1`, `c2`. Do not
run workspace-wide builds (e.g. `cargo check --workspace`, full test suites) or any other build/test
command."* (#5). Per-call `verifyCommands` overrides the profile default; same for `gate`.

`statusBlock()` always injects:
> End your final message with a single trailing line that is exactly one of: STATUS: DONE,
> STATUS: DONE_WITH_CONCERNS, STATUS: BLOCKED, STATUS: NEEDS_CONTEXT, or STATUS: ERROR. When you
> need an answer from the orchestrator before you can proceed, put your question in the message body
> and end with STATUS: NEEDS_CONTEXT.

The question body is the message text itself — no extra payload field. `output.ts` parses the
trailing `STATUS:` line into `RunStatus` (including `NEEDS_CONTEXT`).

### 8.3 Status derivation (`output.ts`)
`text = raw.result ?? ""`. Precedence:
1. An explicit trailing `STATUS: <X>` line in `text` (last non-empty line matching
   `/^STATUS:\s*([A-Z_]+)\s*$/`, accepted only if `X ∈ RunStatus`).
2. Else `raw.is_error === false && cleanExit` → `DONE`.
3. Else `ERROR`.

### 8.4 Finalize pipeline (`finalize.ts`, #3/#6/#7)
Given `BackendResult` + context, build `RunOutput` then layer on:
1. base via `toRunOutput`; attach `jobId` if present on ctx; attach `downgraded` if set.
2. **#3** `stderrTail` = last 2048 bytes of stderr, but **only** if `!cleanExit || status==="ERROR"`
   and stderr is non-empty (keep it off the happy path).
3. **#7** if a `gate` is set: run it (section 8.5); a failed gate downgrades a clean `DONE` →
   `DONE_WITH_CONCERNS`.
4. **#6** compute `changeSet = gitDelta(cwd, headBefore)` (null if not a repo); attach if present.
   For `BackendProvided`, resolve the worktree path under `cwd` first when `worktreeName` is set.
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
Prices come from the curated models map via the derived `priceMap` — no bare-id aliases.

### 8.8 cursor-agent binary resolution (`cursor-bin.ts`)
Order: explicit override → `$CURSOR_AGENT_BIN` → `which cursor-agent` → fallback
`~/.local/bin/cursor-agent`.

### 8.9 Doctor (`doctor.ts`)
Pure parsers + injectable command runner + probes assembled by `runDoctor`:

**Parsers**
- `parseAbout(stdout)` — the real CLI prints whitespace-aligned columns
  (`Field` + 2+ spaces + `Value`), **not** `Label: value`. Match
  `/^(\S.*?\S)\s{2,}(\S.*?)\s*$/` per line; look up (case-insensitive keys) user email, account
  plan/subscription, and current model. (Verified against cursor-agent 2026.07 — do not switch back
  to a colon parser.)
- `parseModelsList(stdout)` — take the first token matching `/^([a-z0-9][a-z0-9._-]*)\b/` on each
  non-empty line (skips "Available models" headers and uppercase-leading prose); dedupe.
- `diffConfiguredModels(configuredIds, accountIds)` — configured ids absent from the account list,
  sorted.

**Runner:** `defaultRunAgentCommand` = promisified `execFile` (never throws; 15s timeout, 2 MiB
buffer).

**Probes**
- `probeAgentVersion(bin)` → `cursor-agent --version`.
- `probeAccount(bin)` → `cursor-agent about`; `loggedIn = (email !== null)`.
- `probeModelMenu(bin, configuredIds)` → try `models`, fall back to `--list-models`; on failure
  return `accountIds: null` + error (caller treats as warning).

**`runDoctor({ config, deep?, ...injectables })`**
- `deep` is reserved and ignored.
- Read plugin version from `package.json` (failure → failure string).
- Resolve bin; if not found → early return with agent/account/modelMenu skipped errors and a
  failure.
- Else probe version (error → failure), account (not logged in → failure), model menu (error or
  each missing configured id → **warning**).
- `ok = failures.length === 0`. Note that prices are not checkable via CLI.

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
`dispatch`/`wait*`/`answerDelegation`; `extra.signal` is forwarded as the abort signal.

---

## 10. Server wiring & lifecycle (`index.ts`)

- Build paths relative to the module (`dist/..` or `src/..`) to locate bundled `config/models.json`.
- `loadConfig({ modelsPath })`; read `~/.cursor/cli-config.json` (tolerate missing — fail-closed
  handled per-call).
- `makeJobRegistry({ backend: makeCursorAdapter(), deadlineMs: cfg.profile.deadlineMs ?? 60000,
  idleMs: cfg.profile.idleMs === undefined ? 180000 : cfg.profile.idleMs })`.
- `deps = { config, registry, cliConfig, serverCwd: process.cwd() }`.
- MCP `Server({name:"cursor-delegate", version:"0.1.0"}, {capabilities:{tools:{}}})`:
  - `ListTools` → `buildTools(deps.config)` (8 tool descriptors; dynamic `model` enum + blurb).
  - `CallTool` → validate required fields per tool, derive the progress sink + abort signal from
    `extra`, route via `handleCall` to `runDelegation` / registry methods / `answerDelegation` /
    `runDoctor`, wrap the result as a JSON text-content block.
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
| #9 | Per-call `idleMs`/`toolIdleMs` overrides; tiered idle watchdog (`phase`-aware: wider window while a tool call is in flight); raw stdout `activity` also re-arms; CANCELLED/STALLED still compute the change-set (`finalizeStall`, skips only gate + incomplete-commit) and `text` summarizes the job's last known progress instead of being empty | `job-registry.ts`, `stream.ts`, `backends/cursor.ts`, `finalize.ts`, `runner.ts`, `tool-schemas.ts` |

---

## 12. Agent catalog & delegate skill (convention layer, not code)

`config/agents/catalog.md` documents reusable "agents" as **convention tuples over `cursor_run`** —
the controller (the host agent) constructs each call; nothing here is Cursor-side config. Columns
are `model` + `requireNonClaude` (not symbolic selectors). The governing principle: *never review an
agent's output with the same model that produced it.* Reviewer roles pass `requireNonClaude: true`
alongside a non-Claude allow-list model. Example tuples: **Verifier** (`gpt-5.5-high`,
`requireNonClaude: true`, `ask`), **Triager** (`composer-2.5`, `ask`), **Design-critic**
(`gemini-3.5-flash`, `requireNonClaude: true`, `plan`), **Codemod** / **Re-implementer** /
**SP-implementer** (`composer-2.5` / `grok-4.5-xhigh`, `write`, `CallerProvided`), **SP
spec/quality reviewers** (different allow-list engines from the implementer, `requireNonClaude:
true`). Plan-writing uses `grok-4.5-xhigh` with `ask`/`plan`. Reproduce this as documentation, not
logic.

`skills/delegate/SKILL.md` (+ `reference.md`) is the orchestration playbook: when to
delegate, model picks, the plan / `NEEDS_CONTEXT` / `cursor_answer` resume flow, and the plan-writer
brief template. Ship it with the plugin; it is not executed by the server.

---

## 13. Packaging & install

### 13.1 As a Claude Code plugin
```jsonc
// plugin.json in .claude-plugin/
{ "name":"cursor-delegate", "version":"0.2.0", "description":"...", "skills":["./skills/delegate"] }
// marketplace.json in .claude-plugin/ (cursor-delegate-local)
// .mcp.json at repo root
{ "mcpServers": { "cursor-delegate": {
    "type":"stdio", "command":"node", "args":["${CLAUDE_PLUGIN_ROOT}/dist/index.js"], "timeout":600000 } } }
```
Install: `claude plugin marketplace add ./` then `claude plugin install cursor-delegate@cursor-delegate-local`.

### 13.2 Build & register (`bin/setup.sh`, portable Linux/macOS)
1. Resolve **this machine's** `node` (`command -v node`) — never bake a path.
2. Warn if `cursor-agent` not on PATH (prerequisite: installed + `cursor-agent login`).
3. `npm install && npm run build` (`tsc` → `dist/`; pure JS, no native deps).
4. Scaffold a **minimal** `~/.config/cursor-delegate/host-profile.json` (`requiredDeny: []` and
   empty policy defaults) **only if absent**; never overwrite an existing profile or the cli-config
   deny-list. Optional keys `default` / `models` extend or override the bundled allow-list.
5. Install plugin at user scope: `claude plugin marketplace add ./ --scope user` then
   `claude plugin install cursor-delegate@cursor-delegate-local --scope user`. `DRY_RUN=1` previews
   without changes.

### 13.3 Manual install (no `claude` CLI)
Add the stdio entry from `.mcp.json` to your MCP client's config directly (resolve
`${CLAUDE_PLUGIN_ROOT}` to the installed plugin cache path).

### 13.4 package.json essentials
```jsonc
{ "type":"module",
  "bin": { "cursor-delegate-mcp":"dist/index.js" },
  "scripts": { "build":"tsc",
               "test":"node --import tsx --test \"tests/**/*.test.ts\"",
               "test:live":"CURSOR_DELEGATE_LIVE=1 node --import tsx --test \"tests/**/*.live.test.ts\"" },
  "dependencies": { "@modelcontextprotocol/sdk":"^1.0.0" },
  "devDependencies": { "tsx":"^4", "typescript":"^5.6", "@types/node":"^22" } }
```
`tsconfig`: `target ES2022`, `module/moduleResolution NodeNext`, `strict`, `rootDir src`,
`outDir dist`.

---

## 14. Prerequisites (per machine, not bundled)
1. `cursor-agent` CLI installed and logged in (`cursor-agent status` / `cursor-agent about`).
2. A host profile at `~/.config/cursor-delegate/host-profile.json` (scaffolded by setup if absent).
3. The host deny-list merged into `~/.cursor/cli-config.json` `permissions.deny` (required for any
   write capability).

---

## 15. Testing strategy

Every impurity is injected, so units test against fakes: a `spawnFn` that replays canned
stream-json lines, a fake `Clock` (`now()` + `setTimer`) to drive deadline/idle/timeout
deterministically, an injected `finalize`, and an in-memory config reader. Doctor probes take an
injected `runCommand`. There is one live integration test (real `cursor-agent`, opt-in via
`npm run test:live`). Coverage mirrors the module list: capability, isolation, safety, prompt,
models, pricing, output, git, gate, stream, config, tool-schemas, doctor, job-registry (incl.
lookupAnswer), cursor adapter/bin, finalize, progress, runner (runDelegation + answerDelegation),
plus an `index` smoke test.

Key invariants worth asserting when reproducing:
- write capability with a missing deny pattern **throws before spawn**;
- unknown model id → `ModelNotAllowedError`; `requireNonClaude` + Claude family →
  `NonClaudeViolationError` (no silent swap); omitted model resolves to `config.default`;
- schema `model` enum + recommended blurb are generated from config;
- a job that exceeds the deadline returns `{RUNNING, jobId}` and is later pollable to terminal;
- a second write to a locked `CallerProvided` path returns `{BUSY}`;
- idle watchdog SIGTERMs and marks `STALLED`; cancel marks `CANCELLED`; both survive finalize;
- `stderrTail` present only on non-clean exit; `gate` failure downgrades `DONE`→`DONE_WITH_CONCERNS`;
- incomplete-commit (commits + dirty tree) downgrades unless `allowPartialCommit`;
- trailing `STATUS: NEEDS_CONTEXT` parks a job answerable via `cursor_answer`; unknown jobId →
  `{NOT_FOUND}`; job not awaiting input → rejected;
- doctor: missing binary / not-logged-in are failures; configured-but-missing menu ids are warnings;
  `ok === failures.length === 0`.
