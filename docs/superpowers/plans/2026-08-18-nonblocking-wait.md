# Plan: a non-blocking wait UX for cursor-delegate jobs

Spec: `docs/superpowers/specs/2026-08-18-nonblocking-wait.md`

## Global constraints

- **Module system**: this repo is ESM (`"type": "module"` in `package.json`). Every new or
  moved TypeScript file uses `.js`-suffixed relative imports, matching `job-registry.ts` /
  `doctor.ts` today.
- **No new runtime dependencies.** The status-record writer and the plugin-registration check
  use only `node:fs`, `node:os`, `node:path` — no new package.json dependency.
- **Injectable-deps testing convention.** Every new piece of impure I/O (file writes, file
  reads, `existsSync`) is exposed as a constructor-injected function with a real default,
  mirroring `Clock`/`finalize`/`finalizeStall` in `job-registry.ts` and `binExists`/
  `runCommand`/`readPackageVersion` in `doctor.ts`. Tests inject fakes; nothing new touches the
  real filesystem in a unit test.
- **Validation commit**: `a3a73db` (`fix(job-registry): tiered idle watchdog and honest
  STALLED/CANCELLED output`). Verified identical on Mac (`/Users/amogh.akshintala/cursor-delegate`)
  and arca (`/home/amogh.akshintala/cursor-delegate`) on 2026-08-18; working tree clean on both
  (Mac has one untracked file, the spec itself). `ship` re-validates premises below if HEAD has
  moved past this commit on either host.
- **Plugin manifest convention** (verified against three working local installs — `forge-local`
  at `~/work/forge/.claude-plugin/{plugin.json,marketplace.json}`, `cairn-local` at
  `.../cairn/plugin/.claude-plugin/marketplace.json` + `.../cairn/plugin/cairn/.claude-plugin/plugin.json`,
  and Anthropic's own `example-plugin` template — plus two internal Databricks stdio-MCP
  plugins, `ice` and `money-dev-productivity`, both of which use
  `${CLAUDE_PLUGIN_ROOT}`-relative `command`/`args` in a plugin-root `.mcp.json`):
  - `.claude-plugin/plugin.json` + `.claude-plugin/marketplace.json` live at the plugin root
    (the *source* directory named by the marketplace's `source: "."`).
  - An MCP server plugin ships a `.mcp.json` at the plugin root: `{"mcpServers": {"<name>": {
    "type": "stdio", "command": ..., "args": [...] }}}`, with any plugin-relative path written
    as `${CLAUDE_PLUGIN_ROOT}/...`. Claude Code auto-discovers this file; nothing in
    `plugin.json` needs to reference it.
  - Registration is done through the `claude` CLI, never by hand-editing JSON:
    `claude plugin marketplace add <path>`, `claude plugin install <name>@<marketplace>`,
    `claude plugin validate <path> --strict`. Removing the legacy raw registration is
    `claude mcp remove <name>`.
  - **`${CLAUDE_PLUGIN_ROOT}` is the *installed cache copy* of the plugin, not the source
    repo.** Confirmed empirically: `forge@forge-local`'s install at
    `~/.claude/plugins/cache/forge-local/forge/0.3.2` differs from
    `~/work/forge` (source) — it's missing three files added to the source since install, and
    it carries one stray file (`.dispatch.md.swp`) no longer present in source. That's a plain,
    non-gitignore-aware, point-in-time recursive copy taken at install/update time, not a live
    reference to the repo. Two consequences for this plan: (a) `dist/index.js` must already
    exist (i.e. `npm run build` already run) *before* `claude plugin install` runs, since
    whatever's on disk at that moment is what gets copied; (b) a later rebuild of `dist/` does
    **not** propagate to the installed copy without re-running `claude plugin update` (or a
    reinstall) — Task 5 makes both of these explicit steps, not assumptions.
  - Reachability ground truth for "which server does the name `<X>` currently resolve to" is
    `claude mcp get <X>` — verified: it exits 1 with `No MCP server named "<X>"...` when
    nothing is registered under that name, and exits 0 with `Status: Connected` plus the
    resolved `Command`/`Args` when something is. This is authoritative in a way that
    reconstructing the answer from `installed_plugins.json` + `.mcp.json` + `settings.json`
    is not (those files describe what *should* resolve; `claude mcp get` reports what actually
    does), so Task 3's diagnostic is built on it rather than on hand-parsed config files.
  - **Known, accepted side effect, out of scope for this plan**: plugin-sourced MCP servers
    appear to get a different tool-facing name prefix than raw-registered ones — this
    session's own tool list shows the plugin-installed `cairn` server's tools as
    `mcp__plugin_cairn_cairn__*`, not `mcp__cairn__*`. cursor-delegate's tools will likely
    rename similarly post-migration (e.g. `mcp__cursor-delegate__cursor_run` →
    `mcp__plugin_cursor-delegate_cursor-delegate__cursor_run`), which would affect every
    existing reference to the short name (forge's `dispatch.md`, skills, memory). Task 5 notes
    this as an observed fact to confirm, not a defect to fix here — updating those other
    references is a separate follow-up if the rename is confirmed.

## Premises verified against the repo

- `src/job-registry.ts`'s `Job` type and `makeJobRegistry` closure hold all state in-memory
  (`Map`s `active`/`completed`); no persistence exists today. Confirmed by reading the full
  file — no `fs` import, no disk I/O anywhere in it.
- `poll(jobId)` already computes exactly the payload the spec wants persisted: a `{status:
  "RUNNING", progress}` snapshot while running, or `{status, result}` (the full `RunOutput`)
  once terminal — including `NEEDS_CONTEXT`, since `retire()` is the single terminal-transition
  path for every `RunStatus` value, `NEEDS_CONTEXT` included. So "write the full terminal
  result" and "write at exactly two points" both reduce to: call the existing `poll(jobId)` and
  persist its return value, once right after `active.set` in `dispatch()`, once at the end of
  `retire()`. No new result-shaping logic is needed.
- `cursor-delegate/plugin/plugin.json` currently declares `"mcpServers": ".mcp.json"` — a bare
  string, which matches no field shape used by any working local plugin (object keyed by
  server name, or a separate auto-discovered `.mcp.json` file) or by the official
  `mcp-integration` skill's documented process. Combined with the wrong manifest location
  (`plugin/plugin.json` instead of `.claude-plugin/plugin.json`), this is why the scaffold has
  never actually registered — confirmed by checking `~/.claude/plugins/known_marketplaces.json`
  and `~/.claude/settings.json` on Mac and arca: neither lists a `cursor-delegate-local`
  marketplace or `cursor-delegate@cursor-delegate-local` plugin; `cursor-delegate` is only
  reachable today via a raw `mcpServers` entry in `~/.claude.json` on both hosts.
- `dist/index.js` exists at the repo root on both hosts (built output of `src/index.ts`), so
  once the manifest moves to repo-root `.claude-plugin/`, the plugin-root-relative path is
  `${CLAUDE_PLUGIN_ROOT}/dist/index.js` (no `/../`, unlike the current scaffold's
  `plugin/.mcp.json` which needs the `/../` only because `plugin/` is one level below repo
  root).
- `doctor.ts`'s existing `RunDoctorOpts`/`DoctorReport` shapes already separate "what's probed"
  from "how", with every probe injectable (`resolveBin`, `binExists`, `runCommand`,
  `readPackageVersion`) and `warnings`/`failures` arrays accumulated by the caller — the new
  plugin-registration check slots into this same shape rather than inventing a new report
  format.

## Task 1 — Status-record persistence — **DONE** (2026-08-18)

Implemented by cursor-delegate (`composer-2.5`, session `c22b3dba-c3a6-4c0e-9657-3b6a9f7f1a13`).
Gate green: `npm test` (180 pass, 1 skipped) + `npm run build` clean. Reviewed by
`gpt-5.6-sol-high` (cross-family): one `structural` finding on the `statusWriter` try/catch
wrapper in `makeJobRegistry` — traced against the actual call graph (`retire()` runs inside
`finalizeJob`/`finalizeJobError`, which back `job.completion`, unawaited on the `background:
true` path) and confirmed the wrapper is a *necessary* resolution of a genuine self-contradiction
in this task's own text (which says "no try/catch at the call site" in one place while requiring
"no unhandled exception/rejection" from an injected throwing writer in another) — kept as
implemented, not treated as a defect. Four `local` findings (two tautological assertions that
could never fail, a cwd-independence test that chdir'd into the exact directory it was supposed
to prove independence from, missing coverage for "no write on intermediate progress events",
hardcoded job IDs risking cross-run collisions) were all real and fixed in one round; still
uncommitted on disk (`src/job-registry.ts`, `tests/job-registry.test.ts`, new `src/status-record.ts`).

## Task 1 — Status-record persistence

**Delivers**: every dispatched job's registry-tracked status is mirrored to a per-job file on
disk, written at exactly the job's start and its terminal transition, containing exactly what
`poll(jobId)` would return at that moment.

**Files**: `src/job-registry.ts` (edit); `tests/job-registry.test.ts` (add cases); optionally a
new `src/status-record.ts` if the writer implementation is cleaner split out (implementer's
call — keep `job-registry.ts` the single source of truth for *when* writes happen either way).

**Contracts**:

```ts
// New export from job-registry.ts (or status-record.ts, re-exported)
export interface StatusRecordWriter {
  /** Synchronous and MUST NOT throw. Any internal failure (fs error, an async write's
   * rejection) is caught and swallowed inside the implementation itself — the call site never
   * wraps this in try/catch and never awaits anything from it. This is the single place
   * "best-effort" is enforced; the registry treats every `write` call as infallible. */
  write(jobId: string, record: PollResult): void;
}

/** Default: JSON file at join(os.tmpdir(), "cursor-delegate-jobs", `${jobId}.json`).
 * Deliberately ignores `process.cwd()` and any caller-supplied working directory — the path is
 * a pure function of jobId. Every call does `mkdirSync(dir, { recursive: true })` (idempotent
 * and race-safe for concurrent same-process calls — never special-cases EEXIST as fatal), then
 * writes to a temp file in the same directory and `renameSync`s it into place, so a concurrent
 * reader (Task 4's poll loop) never observes a truncated or partially-written JSON file. The
 * whole body is wrapped in one try/catch that swallows any error (ENOSPC, EACCES, ...) — this
 * function never throws, per the `StatusRecordWriter` contract above. */
export function fileStatusRecordWriter(): StatusRecordWriter;
```

- `RegistryDeps` (in `job-registry.ts`) gains an optional field: `statusWriter?:
  StatusRecordWriter`. `makeJobRegistry` defaults it to `fileStatusRecordWriter()` when omitted,
  exactly like `clock`/`finalize`/`finalizeStall` are defaulted today.
- In `dispatch()`: immediately after `active.set(jobId, job);`, call
  `statusWriter.write(jobId, poll(jobId));` as a bare statement — no try/catch needed at the
  call site, since the contract above makes `write` infallible by construction. `poll` is
  already defined via `function` hoisting in the same closure, so ordering in the file doesn't
  matter.
- In `retire(job, out)`: as the last statement (after `completed.set(job.id, job)` and the
  `COMPLETED_CAP` eviction loop), call `statusWriter.write(job.id, poll(job.id));` as a bare
  statement, same reasoning. Since `retire` is `makeJobRegistry`'s single terminal-transition
  function — called from `finalizeJob`, `finalizeJobError`, and nowhere else — this one call
  site covers every terminal `RunStatus` (`DONE`, `DONE_WITH_CONCERNS`, `BLOCKED`,
  `NEEDS_CONTEXT`, `ERROR`) and every `terminationReason` (`CANCELLED`, `STALLED`).
- The BUSY early-return path in `dispatch()` (same-path write lock) creates no `Job` and must
  not write a record — it returns before `active.set` is reached, so this holds automatically
  as long as the write call is placed after `active.set`, not before.
- **`tests/helpers.ts`'s `setup()` helper must default `statusWriter` to a no-op** (`{ write: ()
  => {} }`) in the object passed to `makeJobRegistry`, unless the test's own override object
  supplies one. Every *existing* test in `job-registry.test.ts` goes through this helper and
  currently exercises none of this — without a no-op default, every existing test would start
  performing real filesystem writes under `os.tmpdir()` the moment `fileStatusRecordWriter()`
  becomes the registry's own default. Only the new persistence-specific tests below override it
  with a spy.
- `fileStatusRecordWriter()`'s own direct test (last bullet below) is a deliberate, narrow
  exception to this plan's "nothing new touches the real filesystem in a unit test" global
  constraint: it is the one test of the real default adapter itself, exactly analogous to how
  `defaultRunAgentCommand`/`defaultReadPackageVersion` in `doctor.ts` are real-I/O defaults that
  aren't unit-mocked either. Every other test in this task uses the injected spy.

**Test seam and cases** (`tests/job-registry.test.ts`, using the existing `FakeClock` /
`makeFakeBackend` / `specOf` helpers from `tests/helpers.ts`):

- Inject a spy `StatusRecordWriter` (an array the test pushes `{jobId, record}` onto) via
  `RegistryDeps.statusWriter` in the existing `setup()` helper's override object.
- A dispatched job produces exactly 2 writes across its lifecycle (one at dispatch, one at
  `handles[0].finish(doneOk)` + flush) — assert `spy.length === 2` for the happy-path DONE case.
- The first write's `record` equals `{status: "RUNNING", progress: {...}}` with `jobId`
  matching the dispatched job's id.
- The second write's `record` equals exactly what `registry.poll(jobId)` returns immediately
  after — i.e. `{status: "DONE", result: <the same RunOutput the test already asserts today>}`.
- A `background: true` dispatch still produces exactly 2 writes (start + terminal), even though
  the caller never awaits completion — drive it with `handles[0].finish(...)` + `flush()` as the
  existing background test does, then assert the spy.
- The idle-watchdog STALLED path (mirroring the existing "idle watchdog SIGTERMs a silent job"
  test) produces exactly 2 writes, and the terminal one has `status: "STALLED"`.
- `cancel()` produces exactly 2 writes total (dispatch + the terminal write inside `retire`
  triggered by the child's `close` after `kill`), terminal one `status: "CANCELLED"`.
- A dispatch attempt that returns `BUSY` (same-path lock held) produces **0** writes for that
  call — assert the spy is unchanged after the second `dispatch()` in the existing
  "second write to a locked path returns BUSY" test.
- **A writer that throws does not propagate**: inject a spy `write` that throws on its second
  call (the terminal one), dispatch a job to completion, and assert the returned `RunOutput` is
  unaffected and no unhandled exception/rejection occurs (Node's `--test` runner fails on
  unhandled rejections by default, so this is a real assertion). This test exists to prove the
  *contract* — a conforming `StatusRecordWriter` never throws — is actually load-bearing at both
  call sites, not to justify adding try/catch around the bare `write(...)` statements themselves
  (there isn't one, per the contract above).
- **The same file is genuinely overwritten from the start record to the terminal one** — not
  silently deduped or ignored on the second write to an existing path. Using the *real*
  `fileStatusRecordWriter()` (not a spy) wired into a real registry: dispatch a job, read the
  file at its expected path and assert its `status` is `"RUNNING"`; finish the job and flush;
  re-read the *same path* and assert its `status` is now the terminal one and its `result`
  matches `poll(jobId)`. This directly catches an implementation that creates the file once and
  silently no-ops on a second write to the same path.
- `fileStatusRecordWriter()` itself, tested directly (no registry) — the one real-filesystem
  test in this task, per the exemption noted above: two calls to `write` with different
  `jobId`s produce two independent files under `os.tmpdir()`, each named by its `jobId` and
  containing the exact JSON of the `record` passed in; the path does not depend on
  `process.cwd()` (assert by chdir-ing the test process, or by constructing the writer and
  checking the observed path is `join(os.tmpdir(), "cursor-delegate-jobs",
  \`${jobId}.json\`)` regardless); a second `write` call for the *same* `jobId` with different
  content fully replaces the file's content (proves the temp-file-then-rename replace path,
  independent of the registry-level test above).

**Verification command**: `npm test` (from repo root on either host) and `npm run build`
(catches any TS type error introduced by the new exports).

**Dependencies**: none.

---

## Review boundary — after Task 1

The status-record mechanism is a complete, independently reviewable unit: new persisted I/O on
every job's lifecycle, on the hot path of `dispatch`/`retire`. Review here before moving on to
the plugin-packaging tasks, which don't depend on this task's internals but do depend on its
existence (Task 4's docs describe this record's shape and location).

---

## Task 2 — Plugin manifest restructure — **DONE** (2026-08-18)

Implemented by cursor-delegate (`composer-2.5`, session `756e5b69-8a52-4e8d-98d3-007bd3f1d5bf`).
Gate green: `claude plugin validate . --strict` (✔ Validation passed) + the narrow grep, both
clean. Reviewed together with Task 3 below.

## Task 2 — Plugin manifest restructure

**Delivers**: `claude plugin validate . --strict` passes from the cursor-delegate repo root,
and the plugin is installable as a local-directory marketplace using the same mechanism already
proven for `forge-local` and `cairn-local`.

**Files**:
- Create `.claude-plugin/plugin.json` (repo root):
  ```json
  {
    "name": "cursor-delegate",
    "version": "0.2.0",
    "description": "Delegate coding/research tasks to Cursor's models via the local cursor-agent CLI, with a curated model allow-list, sandboxing, an async job model, needs-input resume, and ground-truth git verification.",
    "author": { "name": "Amogh Akshintala", "email": "amogh.akshintala@databricks.com" },
    "skills": ["./skills/delegate"]
  }
  ```
- Create `.claude-plugin/marketplace.json` (repo root):
  ```json
  {
    "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
    "name": "cursor-delegate-local",
    "version": "0.2.0",
    "description": "Local directory marketplace shipping the cursor-delegate MCP server and its orchestration skill.",
    "owner": { "name": "Amogh Akshintala", "email": "amogh.akshintala@databricks.com" },
    "plugins": [
      {
        "name": "cursor-delegate",
        "description": "MCP stdio server that delegates coding/research tasks to Cursor's models via the local cursor-agent CLI.",
        "source": ".",
        "category": "workflow",
        "version": "0.2.0"
      }
    ]
  }
  ```
- Create `.mcp.json` (repo root), replacing `plugin/.mcp.json`'s content with the corrected
  plugin-root-relative path *and* the `timeout` the current live raw entry carries (confirmed
  by reading it directly: `~/.claude.json`'s `mcpServers.cursor-delegate.timeout` is
  `600000` — dropping it would let the MCP client's own timeout diverge from
  `cursor_wait`/`cursor_wait_any`/`cursor_wait_all`'s documented `[1000, 600000]`ms clamp,
  which the spec's non-goals require stay intact):
  ```json
  {
    "mcpServers": {
      "cursor-delegate": {
        "type": "stdio",
        "command": "node",
        "args": ["${CLAUDE_PLUGIN_ROOT}/dist/index.js"],
        "timeout": 600000
      }
    }
  }
  ```
- `git mv plugin/skills/delegate skills/delegate` (repo root now has `skills/delegate/SKILL.md`
  and `skills/delegate/reference.md`, content unchanged by this task).
- Delete `plugin/plugin.json`, `plugin/.mcp.json`, and the now-empty `plugin/` directory.
- **Update every reference to the old `plugin/` layout so nothing resurrects it**:
  - `README.md` — its link to `./plugin/skills/delegate/SKILL.md` (and any other `plugin/`
    path) → `./skills/delegate/SKILL.md`.
  - `config/agents/catalog.md` — same path update.
  - `spec.md` — same path update wherever it references the old scaffold location.
  - `bin/setup.sh` — this currently does `claude mcp remove cursor-delegate` /
    `claude mcp add cursor-delegate -- node <path>/dist/index.js`: i.e. it *is* the legacy raw
    registration, scripted. Left as-is, anyone who re-runs it undoes this entire migration.
    Rewrite it to drive the new flow instead (`claude plugin marketplace add ./` +
    `claude plugin install cursor-delegate@cursor-delegate-local`), or — if the implementer
    judges a full rewrite out of scope for this task — reduce it to printing a pointer at the
    new flow and exiting, rather than silently re-adding the raw entry. Either way, after this
    task, running `bin/setup.sh` must not recreate the registration Task 5 removes.
- No change to `src/`, `dist/`, `config/agents/catalog.md`'s content beyond the path fix,
  `tests/` — this task otherwise touches only manifest/skill location and the stale references
  listed above.

**Test seam**: no seam — this is static manifest/file-layout content, not executable logic.
The verification command *is* the check: `claude plugin validate . --strict` is a real
schema/structure validator, not a stand-in for a test.

**Verification command**: `cd <repo root> && claude plugin validate . --strict` — must exit 0
with no errors or warnings. Additionally, `grep -rn "plugin/skills\|plugin/\.mcp\|plugin/plugin\.json" README.md config/agents/catalog.md spec.md bin/setup.sh` must return nothing.

**Dependencies**: none.

---

## Task 3 — Diagnostic self-check — **DONE** (2026-08-18)

Implemented by cursor-delegate (`composer-2.5`, session `d3e66432-426f-4d34-bef3-c16d5e522f61`).
Gate green: `npm test` (191 pass, 1 skipped) + `npm run build` clean. Reviewed together with
Task 2 by `gpt-5.6-sol-high` (cross-family): one `structural` note (the plan's own grep gate for
Task 2 coincidentally matches the *correct* new `.claude-plugin/plugin.json` path since it
contains `plugin/plugin.json` as a substring — checked with a broader grep and confirmed no real
stale reference slipped through; the gate itself is just fragile, not wrong here) and several
`local` findings, three of which were real and fixed in one round: (1) a bug I found and verified
myself outside the review — `claude plugin marketplace add .` (bare dot) is rejected by the real
CLI with "Invalid marketplace source format"; four human-facing instructional strings across
README.md/spec.md/bin/setup.sh told a person to copy-paste exactly that broken form (the
*executed* line in `bin/setup.sh` was unaffected, since it already used an absolute path) — fixed
to `./`; (2) `checkPluginRegistration` crashed with a `TypeError` on `settings.json` containing
literal JSON `null` (a legitimate parse success, distinct from "missing" or "corrupt") — guarded;
(3) a successful `claude mcp get` with an unparseable `Args` line was mislabeled with the same
"legacy raw registration is still live" detail message as a confirmed non-plugin path, though the
two cases don't establish the same fact — given a distinct detail message. Two findings were
verified as false alarms rather than fixed: the marketplace-add idempotency concern (empirically
retested — `claude plugin marketplace add ./ --scope user` run twice both times exits 0, second
run reporting "already on disk"; note this test transiently registered the marketplace on this
Mac's live Claude Code config as a side effect and was immediately unregistered via `claude plugin
marketplace remove cursor-delegate-local` before continuing) and the `resolvesToPluginInstall`
cache-path-prefix check being too broad (matches the plan's own literal contract text, and the
server-name-scoped `claude mcp get` call already makes a same-name collision with an unrelated
plugin's cache path implausible in practice). One `local` finding (missing-settings.json case
produces `ok:false` with an empty `detail` array) was left as-is: it exactly matches this task's
own required test case, so changing it would be a plan-level decision, not an implementer bug —
noted here rather than silently fixed or silently ignored.

## Task 3 — Diagnostic self-check

**Delivers**: a new check, reachable through the existing `doctor` MCP tool, that reports
whether the plugin migration actually held — using `claude mcp get <name>` as ground truth for
"what server does this name currently resolve to," not a reconstruction from static config
files (which can only describe what *should* be true, not what actually is — see the
`claude mcp get` premise in Global Constraints).

**Files**: `src/types.ts` (edit — add `PluginRegistrationCheck` alongside the other `Doctor*`
types, matching where every other doctor-report type already lives, and add
`pluginRegistration: PluginRegistrationCheck` to `DoctorReport`); `src/doctor.ts` (edit — add
`checkPluginRegistration` and wire it into `runDoctor`); new test cases in
`tests/doctor.test.ts`.

**Contracts**:

```ts
// src/types.ts — alongside DoctorAccountInfo, DoctorModelMenuInfo, etc.
export interface PluginRegistrationCheck {
  /** settings.json enabledPlugins[pluginId] === true */
  enabled: boolean;
  /** `claude mcp get <serverName>` exited 0 — some server is live under this name, plugin- or
   * raw-sourced. */
  reachable: boolean;
  /** Only meaningful when `reachable`: the resolved Command/Args from `claude mcp get`'s output
   * point into this host's plugin cache directory (`~/.claude/plugins/cache/...`), not a raw
   * working-tree path — i.e. the live server is actually the plugin's, not the legacy entry. */
  resolvesToPluginInstall: boolean;
  ok: boolean;
  /** Human-readable detail lines, one per failing sub-check. */
  detail: string[];
}

// DoctorReport (src/types.ts) gains:
//   pluginRegistration: PluginRegistrationCheck;
// Deliberately NOT folded into `DoctorReport.ok`/`failures` — see wiring note below.
```

```ts
// src/doctor.ts — new exports; reuses the existing RunAgentCommandFn/AgentCommandResult shape
// (already used for shelling out to cursor-agent) to shell out to the `claude` CLI instead.
export interface CheckPluginRegistrationDeps {
  /** Returns the parsed JSON at `path`. `{ exists: false }` iff the file is genuinely absent;
   * a file that exists but fails to parse is a distinct case and must NOT be reported as
   * `{ exists: false }` (a corrupt settings.json is not the same fact as "no settings.json"). */
  readJson?: (path: string) => { exists: true; value: unknown } | { exists: false } | { exists: true; parseError: true };
  runCommand?: RunAgentCommandFn; // default: defaultRunAgentCommand, invoked with bin="claude"
  homeDir?: string; // default: os.homedir()
  pluginId?: string; // default: "cursor-delegate@cursor-delegate-local"
  serverName?: string; // default: "cursor-delegate"
}

export async function checkPluginRegistration(
  deps?: CheckPluginRegistrationDeps,
): Promise<PluginRegistrationCheck>;
```

Logic:

1. `enabled`: read `${homeDir}/.claude/settings.json` via `readJson`. `true` iff it parsed and
   `.enabledPlugins[pluginId] === true`. A missing file or a parse error both yield `false`
   here (there is no false-positive risk on this sub-check the way there was on the
   now-removed `legacyRegistrationAbsent` static check — "file unreadable" and "key absent"
   both correctly mean "not confirmed enabled").
2. Run `runCommand("claude", ["mcp", "get", serverName])`.
   - Non-zero exit (matches the verified `No MCP server named "<name>"...` case) → `reachable:
     false`, `resolvesToPluginInstall: false`.
   - Zero exit → `reachable: true`; parse the `Command:`/`Args:` lines from stdout (same
     whitespace-column parsing style as `parseAbout`) and set `resolvesToPluginInstall: true`
     iff the resolved path contains `${homeDir}/.claude/plugins/cache/` — i.e. it's inside a
     plugin's installed cache, not a bare repo/working-tree path.
3. `ok = enabled && reachable && resolvesToPluginInstall`; `detail` collects one string per
   failing sub-check (e.g. `"cursor-delegate@cursor-delegate-local is not enabled in
   settings.json"`, `"no MCP server named \"cursor-delegate\" is currently registered"`,
   `"cursor-delegate resolves to a non-plugin path — the legacy raw registration is still
   live"`).

Wiring: `runDoctor` (in `src/doctor.ts`) gains an injected `opts.checkPluginRegistration?: () =>
Promise<PluginRegistrationCheck>` (default: `() => checkPluginRegistration()`), and calls it
**unconditionally at both return points** — including the early return when `cursor-agent`
itself isn't found, since this check has nothing to do with `cursor-agent`'s presence.
`pluginRegistration` is included in the returned `DoctorReport` either way. Its `detail` lines
are **not** pushed into the top-level `failures` array and its `ok` does **not** factor into the
top-level `DoctorReport.ok`: this check reports on a migration that may legitimately not be
complete yet (e.g. mid-rollout in Task 5), and folding it into the main health gate would make
an otherwise-healthy, not-yet-migrated host report as broadly unhealthy. It's surfaced as its
own field, read on demand, not as a new failure mode of the existing `doctor` contract. (This
also means every *existing* `doctor.test.ts`/`index.test.ts` assertion on `report.ok` and
`report.failures` needs no change — they were never touching this field.)

**Test seam and cases** (`tests/doctor.test.ts`):

- `checkPluginRegistration` with injected `readJson`/`runCommand` stubs:
  - `enabledPlugins[pluginId]: true` + `runCommand` stubbed to a successful `claude mcp get`
    whose stdout's `Args:` line contains `.claude/plugins/cache/` → `ok: true`, `detail: []`.
  - `enabledPlugins` missing the key, or `false` → `enabled: false`, present in `detail`,
    `ok: false`.
  - `readJson` returning `{ exists: false }` for settings.json → `enabled: false`, no throw.
  - `readJson` returning `{ exists: true, parseError: true }` for settings.json → `enabled:
    false`, distinct detail message from the missing-file case, no throw.
  - `runCommand` stubbed to the verified non-zero-exit "no such server" shape →
    `reachable: false`, `resolvesToPluginInstall: false`, `ok: false`.
  - `runCommand` stubbed to a successful `claude mcp get` whose `Args:` line is a bare
    working-tree path (no `.claude/plugins/cache/`) → `reachable: true`,
    `resolvesToPluginInstall: false`, `ok: false` — this is the "legacy entry still live" case.
- `runDoctor` with `opts.checkPluginRegistration` stubbed to a `!ok` result:
  - asserts `report.pluginRegistration.ok === false`.
  - asserts `report.ok` and `report.failures` are **unaffected** (still whatever the
    cursor-agent-related probes alone would produce) — this is the assertion that actually
    catches an implementation that wires the check into the wrong report fields.
- `runDoctor` on the early-return path (binary not found, per the existing "cursor-agent not
  found" test in `tests/doctor.test.ts`): asserts `report.pluginRegistration` is present
  (computed, not skipped) even though `report.agent.found === false`.

**Verification command**: `npm test` and `npm run build`.

**Dependencies**: Task 2 (the default `pluginId`/`serverName` only match reality once Task 2's
manifests exist with those names).

---

## Review boundary — after Tasks 2 and 3

Both are code-complete plugin-packaging work (manifest files, one new doctor check) with no
live host mutation yet. Review before Task 5, which is the only task in this plan that touches
either machine's actual Claude Code configuration — everything up to here is a repo diff.

---

## Task 4 — Skill documentation: the wait pattern — **DONE** (2026-08-18)

Implemented by cursor-delegate (`composer-2.5`, session `884be35e-99b4-4609-888e-50189d3b268a`).
No automated gate (documentation, per the plan's own "no seam" call) — verified manually by
constructing real fake status-record files under `/tmp/cursor-delegate-jobs/` and running both
the single-job and batch example commands verbatim against them; both waited correctly and
printed the terminal JSON on flip, cleaned up afterward. I additionally read the full diff
myself (no dedicated review boundary is marked after this task in the plan) and confirmed the
`./SKILL.md#needs-input-resume-flow` cross-reference anchor actually exists (`## Needs-input
resume flow` at `skills/delegate/SKILL.md:97`). No findings.

## Task 4 — Skill documentation: the wait pattern

**Delivers**: `skills/delegate/reference.md` (and a pointer from `skills/delegate/SKILL.md`)
documents the non-blocking wait pattern — both single-job and batch — alongside the existing
blocking `cursor_wait`/`cursor_wait_any`/`cursor_wait_all` documentation, with a runnable
example for each.

**Files**: `skills/delegate/SKILL.md` (edit — add a short pointer/when-to-use note near the
existing wait-tool guidance); `skills/delegate/reference.md` (edit — add the detailed section).

**Contracts** (documentation content, not code — the "contract" here is what the new section
must say and demonstrate):

- States the status-record's location and shape as produced by Task 1: one file per job at
  `join(os.tmpdir(), "cursor-delegate-jobs", \`${jobId}.json\`)`, containing exactly what
  `cursor_poll` would return for that job at that moment — a `{status: "RUNNING", progress}`
  snapshot while running, the complete terminal payload once done.
- **Single-job pattern**: a runnable example using this environment's own `Bash` tool with
  `run_in_background: true`, wrapped in a bounded outer `timeout` (not an unbounded loop — a
  missing/never-written record file must eventually give up and report, not hang forever):
  `timeout 300 bash -c 'until jq -e ".status != \"RUNNING\"" <file> >/dev/null 2>&1; do sleep
  2; done; cat <file>'`. Notes `jq` as the one new host dependency this pattern assumes, and
  says to check for it (`command -v jq`) before relying on the example verbatim.
- **Batch variant**: the same loop generalized over N job-id files (e.g. iterate the array of
  paths inside the `until` condition, requiring all of them to be non-`RUNNING` before exiting),
  producing one notification for the whole batch instead of N, with the same bounded outer
  `timeout`.
- States that `NEEDS_CONTEXT` is terminal for *this wait* (the record leaves `RUNNING`) even
  though the job itself isn't finished — the caller must recognize `NEEDS_CONTEXT` in the
  printed record and follow up with `cursor_answer`, exactly as the existing blocking-wait docs
  already describe for that status.
- Explicitly states when to prefer this over the existing blocking tools: jobs expected to run
  well under a minute still use `cursor_wait`/`cursor_wait_any`/`cursor_wait_all` directly
  (blocking a short turn is cheap and simpler); jobs expected to run longer use this pattern so
  the orchestrating turn isn't blocked for the whole duration.
- Cross-references rather than duplicates the existing model-pick and delegation-policy
  sections already in `reference.md`/`SKILL.md`.

**Test seam**: no seam — documentation. The check in its place is that the example command
block, run for real against an actual or hand-constructed record file, produces exactly the
behavior described (implementer runs it once as part of writing the task, not as an automated
test).

**Verification command**: manual — implementer pastes and runs both example command blocks
against a real (or manually written) status-record file and confirms each behaves as documented
before marking the task done.

**Dependencies**: Task 1 (the record's location/shape must exist as documented) and Task 2 (this
task edits `skills/delegate/{SKILL.md,reference.md}` at their *post-restructure* path — Task 2
is what moves them there from `plugin/skills/delegate/`; dispatching Task 4 before Task 2 lands
races the same files under two different paths).

---

## Task 5 — Host rollout (operational, not a cursor-delegate dispatch) — **IN PROGRESS** (2026-08-18, Mac)

Progress on Mac: steps 1–4 done (validate ✔, build, marketplace add, plugin install at user
scope). Step 5 restart done. Step 6 ✔ — `claude plugin details` shows `Skills (1) delegate` +
`MCP servers (1) cursor-delegate`. Step 10 observation **confirmed early**: after the restart,
the plugin-sourced tools appear under `mcp__plugin_cursor-delegate_cursor-delegate__*` (and the
skill as `cursor-delegate:delegate`), exactly the rename the plan predicted — the legacy raw
server's tools (`mcp__cursor-delegate__*`) are still present in parallel until step 8.

Step 7 doctor run surfaced a **real bug in Task 3's code that Task 3's own tests could not
catch**: `parseMcpGet` was modeled on `parseAbout`'s whitespace-aligned-column format, but the
actual `claude mcp get <name>` output is indented and colon-delimited (`  Command: <path>` /
`  Args: <path>`). No line matched the old regex, so `args` came back `null` and the doctor
reported "could not parse Args" instead of reading the resolved path. The unit tests passed only
because their `mcpGetStdout()` fixture was hand-built in the same fictional column format the
parser expected. Captured the real output empirically (`claude mcp get cursor-delegate` →
resolved to the legacy working-tree path `~/cursor-delegate/dist/index.js`, confirming the legacy
raw entry is what resolves pre-removal). Fixed via cursor-delegate dispatch (`composer-2.5`,
session `bb4a7f3e-4504-4e20-89ff-625b07a68888`): rewrote `parseMcpGet` to split on the first
colon (values may contain colons), corrected the test fixture to the real format, and added a
regression test pinning the verbatim real output (red-first: failed against the old parser,
green after). Gate green (192 tests, 191 pass, 1 skipped; build clean). Fix is scoped strictly to
`parseMcpGet` + the test file — `parseAbout`, the `PluginRegistrationCheck` shape, `ok` logic,
and detail messages are all untouched. This is exactly the class of thing Task 5 exists to
catch (the review had explicitly deferred "compatibility with actual CLI output" to Task 5).

Cache-refresh note: `claude plugin update` is version-keyed and refused to re-copy (version
still `0.2.0` despite changed `dist/`) — the exact stale-cache trap in Global Constraints.
Refreshed via uninstall + reinstall instead; verified the reinstalled cache's `dist/doctor.js`
carries the fixed parser. Step 8 `claude mcp remove cursor-delegate -s user` then ran cleanly
(legacy raw entry gone from `~/.claude.json`).

### ✅ RESOLVED via inline amendment (user chose "amend inline now", 2026-08-18) — see resolution note at end of this section

### ⛔ STRUCTURAL HALT after step 8 — Task 3's diagnostic design rests on a falsified premise

Removing the legacy entry exposed that **two of Task 3's core assumptions are empirically wrong
on this machine**, so step 9's gate (`pluginRegistration.ok === true`) can never pass as designed:

1. **Wrong `serverName`.** The plugin-sourced MCP server is registered as
   `plugin:cursor-delegate:cursor-delegate`, not the bare `cursor-delegate`. Task 3 defaults
   `serverName: "cursor-delegate"` and calls `claude mcp get cursor-delegate` — which now returns
   `No MCP server named "cursor-delegate"` (the bare name only ever resolved because the *legacy
   raw* entry used it; the plugin system namespaces its servers as `plugin:<mktplace-plugin>:<server>`).
   So the diagnostic reports `reachable: false` / `ok: false` for a server that is in fact live
   and in use.

2. **Falsified `${CLAUDE_PLUGIN_ROOT}` premise → wrong `resolvesToPluginInstall` signal.** Global
   Constraints asserts (verified against `forge@forge-local`) that `${CLAUDE_PLUGIN_ROOT}` is the
   *installed cache copy* under `~/.claude/plugins/cache/`. But for cursor-delegate's
   local-directory marketplace, `claude mcp get plugin:cursor-delegate:cursor-delegate` shows the
   running server resolves `CLAUDE_PLUGIN_ROOT=/Users/amogh.akshintala/cursor-delegate/` — the
   **source repo**, with `Args: /Users/amogh.akshintala/cursor-delegate//dist/index.js`. Task 3's
   `resolvesToPluginInstall` tests the resolved path for `~/.claude/plugins/cache/`, which is now
   permanently `false`. (A cache copy *does* exist and is what I refreshed above — but the running
   server does not execute from it for a local-dir marketplace; it runs live source `dist/`. That
   also means the reinstall-to-refresh dance, while correct for the cache, was not what fixed the
   running server — the running server already had the fixed parser the moment the gate rebuilt
   source `dist/`.)

   Decisive detail: the legacy raw entry pointed at `~/cursor-delegate/dist/index.js` and the
   plugin server points at `~/cursor-delegate//dist/index.js` — **the same dist file**. The
   resolved *path* cannot distinguish plugin-sourced from legacy-raw at all here. The only sound
   ground-truth signal is the `claude mcp get` **`Scope:`** line — plugin-sourced shows
   `Scope: Dynamic config (from command line)` with a `CLAUDE_PLUGIN_ROOT` in its `Environment:`
   block; the legacy raw entry showed `Scope: User config (available in all your projects)` with
   no `CLAUDE_PLUGIN_ROOT`.

**Why this is a structural halt, not an inline fix**: the fix changes Task 3's recorded contract
(the `serverName` default and the `resolvesToPluginInstall` semantics/field meaning) and rests on
correcting a premise the plan recorded in Global Constraints. Per `ship`'s routing, a structural
finding stops shipping and returns to `to-plan` (the *how* is wrong; the spec's *what* — "a
diagnostic confirms the reachable server under the cursor-delegate name is the plugin-sourced
one" — still stands, though its "under the cursor-delegate name" phrasing needs revisiting too).
`checkPluginRegistration`'s `serverName`/`readJson`/`homeDir` injectability means the redesign is
contained, but it is still a plan-level decision. **Surfaced to the user; awaiting direction
before any further host action or Task 3 rework.**

**Migration status is otherwise GOOD on Mac** (only the self-diagnostic is broken): plugin
installed + enabled at user scope; plugin server live and reachable (its tools are in use this
session under `mcp__plugin_cursor-delegate_cursor-delegate__*`); legacy raw entry removed — one
registration remains. arca rollout **not started**. **Nothing is committed on either host.**

**Resolution (inline amendment, cursor-delegate `composer-2.5` session
`8715eadc-7ffc-4f5d-b213-8e197e660a12`; two cross-family review rounds with `gpt-5.6-sol-high`).**
`checkPluginRegistration` redesigned and its contract amended:
- `serverName` default → `plugin:cursor-delegate:cursor-delegate` (Fact A).
- `resolvesToPluginInstall` now means "the live server is plugin-launched", detected by a
  `CLAUDE_PLUGIN_ROOT=` entry **inside the `Environment:` block** of `claude mcp get` output
  (structurally parsed — not a raw-stdout scan, so a `CLAUDE_PLUGIN_ROOT` inside an `Args:` value
  can't false-positive), replacing the falsified cache-path-substring check (Fact B).
- **New `legacyAbsent` field** (a second `claude mcp get` of the bare `cursor-delegate` name,
  expected to exit non-zero) — this is what satisfies the spec FR "the legacy raw registration is
  actually absent (not silently reintroduced)", which the first-cut redesign missed (caught by
  review round 2). Final contract: `ok = enabled && reachable && resolvesToPluginInstall &&
  legacyAbsent`; `pluginRegistration` still excluded from the top-level `DoctorReport.ok`.
- The obsolete `args === null` gate (which would have mis-flagged a plugin-sourced server that
  had `CLAUDE_PLUGIN_ROOT` but no parseable `Args:` line) was removed.
Gate green (197 tests, 196 pass, 1 skipped; build clean); every new/changed test red-first.
Reviewed twice cross-family; round-2 findings (the `legacyAbsent` spec gap, the raw-stdout
false-positive, the `args` gate, and two test-realism issues) all fixed in one round, then I
verified the final `src/doctor.ts` directly. **Amends the Global Constraints `${CLAUDE_PLUGIN_ROOT}`
premise: it is the cache copy for some marketplace types but the SOURCE repo for this
local-directory marketplace — the running server executes `${CLAUDE_PLUGIN_ROOT}/dist/index.js`
from source, which also means a source `npm run build` reaches the running server on restart
without needing a cache refresh.**

**Mac Task 5 COMPLETE ✅ (2026-08-18).** After reinstall + restart, the `doctor` tool reports
`pluginRegistration: { enabled: true, reachable: true, resolvesToPluginInstall: true,
legacyAbsent: true, ok: true, detail: [] }` and top-level `ok: true`, no warnings/failures.
One live registration (plugin-sourced) on Mac; legacy raw entry gone. **Step 10 recorded**: the
plugin's tools appear under `mcp__plugin_cursor-delegate_cursor-delegate__*` and the skill as
`cursor-delegate:delegate` (the predicted rename, now confirmed live and used this session).
Updating the old short-name references (forge's `dispatch.md`, skills, memory) is the separate,
explicitly-out-of-scope follow-up the plan flagged — NOT part of this plan.

**arca Task 5 COMPLETE ✅ (2026-08-18).** Code synced Mac→arca via `rsync -az --delete`
(no external push available; `--delete` correctly removed only the stale `plugin/` tree, `.git`
and `node_modules` excluded); arca then `npm install && npm run build` clean, 197 tests
(196 pass, 1 skipped) — matches Mac. Marketplace add + install at user scope; `plugin details`
shows `Skills (1) delegate` + `MCP servers (1)`; legacy raw user-scope entry removed. Verified by
running the actual amended `checkPluginRegistration` from arca's `dist/` against arca's live
config **from a non-repo CWD**: `{ enabled: true, reachable: true, resolvesToPluginInstall: true,
legacyAbsent: true, ok: true, detail: [] }`. arca's `CLAUDE_PLUGIN_ROOT` also resolves to the
source repo (`/home/amogh.akshintala/cursor-delegate/`), confirming the amended premise on both
hosts.

arca-specific gotchas navigated: (1) bare `claude` on arca resolves to a BROKEN
`/usr/local/bin/claude` stub ("native binary not installed") in non-interactive shells — the
working CLI is `~/.local/bin/claude`, on PATH only interactively (matches prior memory); used its
absolute path / prepended `~/.local/bin` to PATH for the verification. (2) See the CWD limitation
below.

### ⚠️ Known diagnostic limitation discovered during arca verify — `legacyAbsent` is CWD-sensitive

Running `checkPluginRegistration` with CWD **inside the cursor-delegate repo** reports
`legacyAbsent: false` ("a server is still registered under the bare name cursor-delegate"),
because the repo's own root `.mcp.json` (created by Task 2 as the plugin's MCP config) is ALSO
interpreted by `claude` as a **project-scoped** MCP server named `cursor-delegate` whenever
`claude` runs from that directory — so `claude mcp get cursor-delegate` resolves it at project
scope. This is NOT the legacy raw (user-scope) registration and NOT a reintroduction of it; the
user-scope legacy entry is genuinely gone (verified: `claude mcp get cursor-delegate` from any
non-repo dir returns "No MCP server named cursor-delegate"). Impact is narrow and fails safe (it
over-warns, never falsely-greens): only a Claude Code session launched with CWD = the
cursor-delegate repo itself sees the spurious warning; normal use (delegate tools invoked from
other work repos, as on Mac and as in the from-`/tmp` arca check) reads correctly green.
Candidate fix (deferred — surfaced to user): `claude mcp get` output carries a `Scope:` line, so
`legacyAbsent` could be tightened to only count a bare-name resolution whose `Scope:` is
`User config` (the legacy raw entry's scope), ignoring a `Project config` resolution from the
repo's own `.mcp.json`. Not fixed inline pending user decision, since both hosts' migrations are
functionally complete and correct and this is a self-check-precision refinement, not a migration
defect.

**Both hosts' migrations verified green. After-Task-5 review boundary satisfied**: one
plugin-sourced registration per host, legacy raw entries gone, and Task 4's wait-pattern docs
reachable via the now-discoverable `cursor-delegate:delegate` skill (confirmed in `plugin details`
on both hosts). **Nothing committed on either host** (no external push available; hosts kept in
sync via rsync at uncommitted-working-tree parity).

## Task 5 — Host rollout (operational, not a cursor-delegate dispatch)

**Delivers**: exactly one live registration of the `cursor-delegate` MCP server, sourced from
the plugin, on each of Mac and arca; the legacy raw registration removed from each.

This task is **not** dispatched to a cursor-delegate implementer — it mutates each machine's own
Claude Code configuration, which is exactly the kind of hard-to-reverse, host-affecting action
the main session's execution-care guidance calls for confirming before doing. The orchestrating
Claude Code session runs these steps directly (locally on Mac, via `run_command`/SSH on arca),
with explicit confirmation before the `claude mcp remove` step on each host (removal is the one
step that isn't trivially undoable without re-running `claude mcp add` by hand).

**Steps, per host** (Mac first, then arca — never both before confirming the first worked):

1. Confirm Tasks 2–4 are merged and present in that host's checkout of the repo (`git log`
   shows the commits; `claude plugin validate . --strict` passes on that host).
2. `npm run build` on that host's checkout — `dist/index.js` must be current *before* install,
   since the plugin cache install is a point-in-time copy (see Global Constraints); installing
   against a stale or missing `dist/` ships stale or broken code to the cache.
3. `claude plugin marketplace add <absolute path to the repo root on that host>` — registers
   the `cursor-delegate-local` marketplace (name comes from `marketplace.json`, matching the
   `forge-local`/`cairn-local` convention already in use on both hosts).
4. `claude plugin install cursor-delegate@cursor-delegate-local -s user` — installs and enables
   it at **user scope**, matching Task 3's check (which reads user-level `settings.json`).
5. Restart the Claude Code session on that host (plugin/MCP registration changes require a
   restart to take effect, per prior experience with this exact class of change).
6. `claude plugin details cursor-delegate@cursor-delegate-local` — confirm its component
   inventory lists `Skills (1) delegate` and `MCP servers (1)`, i.e. the skill is actually
   packaged and discoverable, not just the server. (The spec requires the skill be "listed,
   description-matched, invocable on demand" — this is the direct check for that, versus only
   checking MCP reachability.)
7. Run the `doctor` MCP tool; confirm `pluginRegistration.enabled === true` and
   `pluginRegistration.reachable === true`. `resolvesToPluginInstall` may still be `false` here
   if `claude mcp get cursor-delegate` still resolves to the legacy raw entry rather than the
   plugin's — that's expected while both are registered; it's checked again after step 8.
8. **After confirming step 7 passed** — `claude mcp remove cursor-delegate -s user` (removes
   the legacy raw `mcpServers.cursor-delegate` entry from that host's `~/.claude.json`).
9. Restart again; run `doctor` once more and confirm `pluginRegistration.ok === true` in full.
10. Note the tool-facing name cursor-delegate's tools now appear under (check this session's
    own tool list, or the equivalent on arca, after the restart in step 9) — per the Global
    Constraints note, it may have changed from `mcp__cursor-delegate__*` to a
    `mcp__plugin_cursor-delegate_cursor-delegate__*`-style prefix. Record what it actually is;
    don't assume. Updating other references to the old name (forge's `dispatch.md`, skills) is
    explicitly out of scope for this plan — file it as a follow-up if the rename is confirmed.

**Test seam**: no seam — this is a live-host operational runbook, verified by the diagnostic
tool built in Task 3, not by an automated test.

**Verification command**: the `doctor` MCP tool's `pluginRegistration.ok === true` on both
Mac and arca is the actual gate; there is no separate CI-style command.

**Dependencies**: Tasks 2, 3, and 4 all merged.

---

## Review boundary — after Task 5

Confirms both hosts are in the end state the spec requires: one live registration each, legacy
entry gone, docs describing a pattern that now actually reaches an agent through normal skill
discovery.

## Self-review

- **Coverage**: every functional requirement in the spec maps to a task — persisted per-job
  record (Task 1), non-blocking single/batch wait documented against that record (Task 4),
  plugin discoverability (Task 2 + Task 5, with Task 5 step 6 now directly checking the skill
  is packaged via `claude plugin details`, not only that the server is reachable),
  single-registration-per-host / legacy-removed (Task 5), diagnostic check (Task 3). The two
  non-functional requirements about write volume and caller-cwd independence are both satisfied
  by Task 1's design (two write points reusing `poll()`; `os.tmpdir()`-only path) rather than
  needing a separate task. The reclamation non-functional requirement is satisfied by the same
  design choice (OS-owned tmpdir, no explicit delete anywhere in this plan). The
  "reproducible identically on every host" plugin-registration requirement is now backed by a
  build-then-install step (Task 5 step 2) rather than assuming the source tree alone is enough.
- **Consistency**: `pluginId` (`"cursor-delegate@cursor-delegate-local"`) and `serverName`
  (`"cursor-delegate"`) in Task 3 match the names Task 2 actually creates (`marketplace.json`'s
  `name` + `plugin.json`'s `name`). Task 3's `resolvesToPluginInstall` check and Task 5's
  rollout steps agree on ground truth (`claude mcp get`) rather than one checking static files
  the other doesn't produce. Task 4's documented record location matches Task 1's
  `fileStatusRecordWriter` path exactly, and Task 4 now depends on Task 2 (shared file paths)
  as well as Task 1. Task 3's `pluginRegistration` field is explicitly excluded from
  `DoctorReport.ok`/`failures`, consistent with Task 5 steps 7–8 expecting a legitimate
  transient `!ok` window mid-rollout.
- **Placeholders**: none. The one item this plan deliberately does *not* resolve —
  cursor-delegate's tools' post-migration name prefix — is recorded as an explicit, out-of-scope
  observation in Global Constraints and as a to-confirm step in Task 5 step 10, not silently
  assumed away.
- **Gate history**: this plan went through one review round (`gpt-5.6-sol-high` +
  `cursor-grok-4.6-xhigh`, parallel, non-Claude per the delegation policy). Both returned
  `DONE_WITH_CONCERNS` with substantive, evidence-backed defects — the `${CLAUDE_PLUGIN_ROOT}`
  premise (grok), the fs-injection contradiction and terminal-overwrite test gap (both), the
  missing `README.md`/`catalog.md`/`spec.md`/`bin/setup.sh` references and the `bin/setup.sh`
  self-undo risk (grok), the dropped `timeout` field and stale-`dist/` install risk (gpt), and
  the reachability-check design gap (both, resolved here by rebuilding Task 3 around `claude mcp
  get` rather than either reviewer's proposed fix). One additional defect — the
  `${CLAUDE_PLUGIN_ROOT}` cache-vs-source distinction and the `claude mcp get` ground-truth
  option — was confirmed by direct empirical check against this machine's actual plugin
  installs (`forge@forge-local`) rather than taken on either model's word. All of the above are
  fixed in this revision. Not re-run as a second review round: none of these defects recurred
  after a first fix (the bar in the `to-plan` skill for escalating to the human), and the
  remaining reviewer disagreements (atomic-write hygiene, file permissions, `jq` as a soft
  dependency, doctor.ok semantics) were either already adopted into the fix (atomic write,
  bounded timeout, ok/failures separation) or are genuinely advisory with no plan change needed.
