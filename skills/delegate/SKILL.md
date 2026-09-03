---
name: delegate
description: >
  Orchestrate coding, research, plan-writing, and review work through cursor-delegate
  MCP tools (cursor_run / cursor_answer). Use when the user asks to delegate to Cursor,
  fan out plan-writing or implementation to a non-Claude model, pick a model for
  cursor_run, resume a NEEDS_CONTEXT parked job, or follow the agent catalog roles
  (Verifier, Triager, Design-critic, Codemod, Re-implementer, SP-implementer,
  SP reviewers).
---

# Delegate (cursor-delegate)

You are the **orchestrator**. Cursor models do the token-heavy delegated work via
`cursor_run`. You review results, answer clarifying questions, and never treat the
catalog as Cursor-side config — it is a convention layer you construct calls from.

**Governing principle:** never review an agent's output with the same model that
produced it. For reviewer roles, pass `requireNonClaude: true` and pick a
non-Claude allow-list model.

## When to delegate

Delegate when the work is:

- **Token-heavy plan authoring** after you (or the user) already have a spec —
  the driving use case: brainstorm/spec stays with you; detailed plan-writing goes
  to `cursor-grok-4.6-xhigh`.
- **Mechanical or well-scoped coding** (codemod, single planned task, rebuild from
  a clear spec).
- **Uncorrelated review** (refute a claim, critique a design, review a spec or
  implementation) on a **different** model than the producer.
- **Bulk triage / classify / summarize** where quality bar is lower than coding.

Do **not** delegate when you still need to invent the product decision yourself,
when the user must stay in the loop on every edit, or when the only available
models would review with the same id that produced the artifact.

## Model picks

Allow-list ids only (server rejects anything else): `composer-2.5`,
`cursor-grok-4.6-xhigh`, `cursor-grok-4.6-high`, `cursor-grok-4.5-high`,
`gemini-3.5-flash`, `gpt-5.6-sol-high`, `gpt-5.6-terra-high`.
`cursor-grok-4.6-high` and `cursor-grok-4.5-high` remain callable; they
are not the plan-writer/coding pick.

| Intent | `model` | `requireNonClaude` |
|---|---|---|
| Bulk / cheap / default | `composer-2.5` | `false` (omit) |
| Plan-writing / strong coding | `cursor-grok-4.6-xhigh` | `false` (omit) |
| Diverse review (uncorrelated) | `gemini-3.5-flash`, `gpt-5.6-sol-high`, or `gpt-5.6-terra-high` | `true` |

Omit `model` only when `composer-2.5` is acceptable — that is the server default.

## Agent catalog (convention tuples)

These "agents" are **convention tuples over `cursor_run`** — you construct each
call. Nothing here is Cursor-side configuration; this is documentation, not code.

| Agent | model | requireNonClaude | capability | isolation | prompt shape |
|---|---|---|---|---|---|
| **Verifier** | `gpt-5.6-sol-high` | `true` | `ask` | None | adversarial: *try to refute the claim / find the bug* |
| **Triager** | `composer-2.5` | `false` | `ask` | None | classify / route / summarize an issue |
| **Design-critic** | `gemini-3.5-flash` | `true` | `plan` | None | critique a design, surface risks, propose alternatives |
| **Codemod** | `composer-2.5` | `false` | `write` | CallerProvided | mechanical, well-scoped edit across a tree |
| **Re-implementer** | `cursor-grok-4.6-xhigh` | `false` | `write` | CallerProvided | rebuild a component from a spec |
| **SP-implementer** | `composer-2.5` | `false` | `write` | CallerProvided | implement a single planned task |
| **SP spec-reviewer** | `gemini-3.5-flash` | `true` | `ask` | None | review a spec for gaps (different model than implementer) |
| **SP quality-reviewer** | `gpt-5.6-terra-high` | `true` | `ask` | None | review an implementation for quality (different model than implementer) |

Notes:

- Implementers use `CallerProvided` isolation so writes land in a known working tree and
  participate in the same-path write lock.
- Always pass `verifyCommands` to bound what an implementer may run, and a `gate` to enforce
  the postcondition the tool itself checks.
- Plan-writing (not a named row above) uses `cursor-grok-4.6-xhigh` with `capability` `ask`
  or `plan`; see [Plan-writer brief](#plan-writer-brief) below.

## Calling `cursor_run`

Minimum shape:

```json
{
  "prompt": "<task for the Cursor agent>",
  "model": "cursor-grok-4.6-xhigh",
  "capability": "ask"
}
```

Common additions:

- `capability`: `ask` | `plan` | `write` | `write-unsandboxed`
- `isolation`: `{ "type": "CallerProvided", "path": "<abs working tree>" }` for writes
- `verifyCommands`: string[] — only verify commands the agent may run
- `gate`: postcondition **you** (the tool) enforce after the agent
- `requireNonClaude`: `true` for reviewer roles
- `background`: `true` to fan out; then wait for completion (see below)

Always end delegated prompts with an instruction to finish with a trailing
`STATUS: DONE` | `BLOCKED` | `NEEDS_CONTEXT` line (the server also injects a
status-convention block; reinforce it in plan-writer briefs).

## Waiting on jobs

### Blocking wait tools (short jobs)

For jobs expected to finish **well under a minute**, block the current turn with the MCP wait
tools — simpler than the file-watch pattern below:

| Tool | Use when |
|------|----------|
| `cursor_wait` | One job; block until it is terminal (or timeout). |
| `cursor_wait_any` | Several jobs; block until the **first** reaches a terminal state. |
| `cursor_wait_all` | Several jobs; block until **all** are terminal. |

Each accepts optional `timeoutMs` (default 120000, clamp `[1000, 600000]`). On timeout they
return the current `RUNNING` snapshot rather than hanging forever.

If the returned status is `NEEDS_CONTEXT`, the `result` carries `jobId` and the delegate's
question in `result.text`. Answer via `cursor_answer` (see
[Needs-input resume flow](#needs-input-resume-flow)) and continue until a fully terminal status.

### Non-blocking wait pattern (long jobs)

For jobs expected to run **longer than about a minute**, do not hold the orchestrating turn
inside `cursor_wait*`. Instead, watch the **status record** the server writes to disk and
run the poll loop in a **background shell** so this turn can end and you are notified once
when the job (or batch) is done.

#### Status record — location and shape

Every dispatched job gets one JSON file:

```
join(os.tmpdir(), "cursor-delegate-jobs", `${jobId}.json`)
```

On macOS `os.tmpdir()` is usually `$TMPDIR` (under `/var/folders/.../T/`); on Linux it is
often `/tmp`. Resolve at runtime with `echo "${TMPDIR:-/tmp}/cursor-delegate-jobs/${JOB_ID}.json"` or `node -e "console.log(require('node:path').join(require('node:os').tmpdir(), 'cursor-delegate-jobs', process.argv[1] + '.json'))" "$JOB_ID"`.

The file contains exactly what `cursor_poll` would return at that moment:

- While running: `{ "status": "RUNNING", "lastHeartbeatAt": <server ms>, "progress": { ... } }`
- When done: `{ "status": "<terminal>", "result": <RunOutput> }` — full terminal payload,
  not just a status label.

The server writes at job start, then **refreshes the record every 30s while running**
(heartbeat), and once more at the terminal transition. `lastHeartbeatAt` is the server clock
at the last refresh: if it stops advancing while the record still says `RUNNING`, the server
died mid-job — treat the job as lost and redispatch rather than waiting forever.

#### Host dependency: `jq`

The examples below use `jq` to test `.status`. It is the one new host dependency this
pattern assumes. Check before running verbatim:

```bash
command -v jq >/dev/null || { echo "jq is required for the status-record wait pattern" >&2; exit 1; }
```

#### Single-job pattern

After `cursor_run` with `background: true`, note the returned `jobId`, set `STATUS_FILE` to
its record path, then launch a **bounded** background wait (missing or never-written files
must not hang forever — the outer `timeout` enforces that):

```bash
JOB_ID="<from cursor_run>"
STATUS_FILE="${TMPDIR:-/tmp}/cursor-delegate-jobs/${JOB_ID}.json"

command -v jq >/dev/null || { echo "jq required" >&2; exit 1; }

timeout 300 bash -c 'until jq -e ".status != \"RUNNING\"" "$1" >/dev/null 2>&1; do sleep 2; done; cat "$1"' _ "$STATUS_FILE"
```

In Claude Code, invoke that shell with the **Bash** tool and `run_in_background: true` so
this turn is not blocked for the full job duration. When the background command completes,
read its stdout — that is the terminal `PollResult` JSON.

#### Batch variant

Same pattern over **N** job ids: the `until` loop requires **every** record to be non-`RUNNING`
before exiting, then prints all terminal records (one notification for the whole batch):

```bash
JOB_IDS=( "<id-a>" "<id-b>" )   # one entry per background cursor_run
FILES=()
for id in "${JOB_IDS[@]}"; do
  FILES+=( "${TMPDIR:-/tmp}/cursor-delegate-jobs/${id}.json" )
done

command -v jq >/dev/null || { echo "jq required" >&2; exit 1; }

timeout 300 bash -c '
  FILES=("$@")
  until
    all=true
    for f in "${FILES[@]}"; do
      if ! jq -e ".status != \"RUNNING\"" "$f" >/dev/null 2>&1; then
        all=false
        break
      fi
    done
    $all
  do
    sleep 2
  done
  for f in "${FILES[@]}"; do
    echo "=== $f ==="
    cat "$f"
    echo
  done
' _ "${FILES[@]}"
```

Launch with Bash `run_in_background: true` as above.

#### `NEEDS_CONTEXT` is terminal for this wait

When the delegate parks for input, the status record leaves `RUNNING` with
`status: "NEEDS_CONTEXT"` and the full `result` (including `jobId` and the question in
`result.text`). The background wait **exits and prints that record** — the job itself is not
finished, but **this wait is**. Inspect the printed JSON; if `status` is `NEEDS_CONTEXT`,
follow up with `cursor_answer` exactly as for blocking `cursor_wait` (see
[Needs-input resume flow](#needs-input-resume-flow)), then wait again
if the answer resumes a still-running job.

After `cursor_answer` resumes the run under a new jobId, the parked record gains
`"supersededBy": "<newJobId>"` — a forward pointer, not a status change. If you were waiting
on the original id, follow the chain: wait on the new id's record instead (and note that the
original record stays `NEEDS_CONTEXT` forever; only its `supersededBy` field moves).

#### When to use which

| Expected duration | Approach |
|-------------------|----------|
| Well under a minute | `cursor_wait` / `cursor_wait_any` / `cursor_wait_all` (blocking) |
| Longer runs | Status-record background shell (non-blocking) |

Blocking tools remain correct and preferred for short work; the file-watch pattern exists so
the orchestrating turn is not tied up for the entire delegate runtime.

## Needs-input resume flow

1. You call `cursor_run` (foreground or `background: true`).
2. If the result `status` is `NEEDS_CONTEXT`, the result always includes a `jobId`
   (parked job). The `text` field **is** the delegate's question — no separate
   question field.
3. Decide the answer yourself (orchestrator), or ask the human if needed.
4. Resume with:

```json
{
  "jobId": "<from the NEEDS_CONTEXT result>",
  "answer": "<your answer>"
}
```

via `cursor_answer`. The return shape matches `cursor_run` (may be terminal,
`NEEDS_CONTEXT` again, or `RUNNING` + `jobId`).

5. Unknown/expired `jobId` → `NOT_FOUND`. A job not awaiting input is rejected
   ("job is not awaiting an answer").

Reliability caveat: detection depends on the model emitting `STATUS: NEEDS_CONTEXT`.
If a weak model skips the line and guesses, your review of the artifact is the
backstop — especially for delegated plans.

## Driving use case: delegated plan-writing

1. You hold the approved spec (brainstorm done).
2. Build the prompt from the **PLAN-WRITER BRIEF TEMPLATE**
   [below](#plan-writer-brief-template) — fill every placeholder; do not send an
   empty template.
3. `cursor_run` with `model: "cursor-grok-4.6-xhigh"`, `capability: "ask"` or `"plan"`
   (read-only plan authoring; use `write` only if the plan must be written into the
   repo by the delegate).
4. On `NEEDS_CONTEXT`, answer via `cursor_answer` and continue until `DONE` /
   `DONE_WITH_CONCERNS` / `BLOCKED` / `ERROR`.
5. Review the plan yourself. For a second opinion, run a Verifier or Design-critic
   catalog role on a **different** model with `requireNonClaude: true`.

## Plan-writer brief

Before any plan-writing `cursor_run`, read and apply the template below
(PLAN-WRITER BRIEF TEMPLATE + example filled brief).

### PLAN-WRITER BRIEF TEMPLATE

Copy everything inside the fence into `cursor_run.prompt` after replacing
placeholders (`«...»`). Do not leave placeholders unfilled. Use
`model: "cursor-grok-4.6-xhigh"` unless the user explicitly overrides with another
allow-list id.

```text
You are a plan-writing delegate. The orchestrator (a different model) already
brainstormed and approved the design. Your job is to write ONE detailed
implementation plan markdown file — planning document only.

## Hard rules

- Do NOT implement code. Do NOT modify source. Do NOT run build/test commands
  except read-only inspection needed to name exact paths (`ls`, `rg`, `Read`).
- Do NOT commit. Do NOT create git commits or PRs.
- Your only deliverable is the plan content (and, if the orchestrator asked you
  to write a file, that single markdown path).
- If you lack a fact that blocks a correct plan (missing path, unclear API,
  ambiguous requirement), stop and ask. End your message with the question as
  the body and a trailing line exactly:

  STATUS: NEEDS_CONTEXT

  The orchestrator will answer via cursor_answer and you will resume. Do not
  guess through blockers.
- When the plan is complete, end with:

  STATUS: DONE

## Output path

Write the plan to: «PLAN_OUTPUT_PATH»
(Example: docs/superpowers/plans/YYYY-MM-DD-«feature-slug».md)

## Plan document header (required)

Start the plan with this header shape (fill Goal / Architecture / Tech Stack):

# «Feature Name» Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** «one sentence»

**Architecture:** «2-3 sentences»

**Tech Stack:** «key technologies»

## Global Constraints

«Copy project-wide rules from the spec verbatim — one line each.»

## Spec / context (authoritative)

«PASTE_APPROVED_SPEC_OR_SUMMARY»

## Assumed already true

«LIST_DEPENDENCIES_ALREADY_SHIPPED — do not re-plan these»

## Out of scope

«LIST_EXCLUSIONS»

## File structure (target)

| Path | Role |
|------|------|
| «path» | «responsibility» |

## Methodology for THIS plan

«If the subsystem is runtime code: use TDD — failing test → run fail → minimal impl → run pass → commit.»
«If the subsystem is documentation-only: replace unit-test steps with VERIFICATION steps (file existence, rg/grep assertions with expected output). Still use bite-sized `- [ ]` steps and commit after each task.»

## Task structure (every task)

### Task N: «name»

**Files:**
- Create: `«exact/path»`
- Modify: `«exact/path»`
- Test or Verify: `«exact/path-or-command»`

**Interfaces:**
- Consumes: «exact names from earlier tasks»
- Produces: «exact names later tasks rely on»

Then bite-sized steps:

- [ ] **Step …:** show the ACTUAL content to write (full markdown/code — no TBD)
- [ ] **Step …:** run the exact verification/test command; state Expected: …
- [ ] **Step …:** Commit with an exact `git add` + `git commit -m "..."` block

## No placeholders in the plan you write

Never leave "TBD", "TODO", "similar to Task N", or "add appropriate error handling"
without the real content. Every step must be executable by an engineer with zero
repo context.

## Self-Review (end of your plan)

After writing all tasks, include a `## Self-Review` section that checks:

1. Spec coverage vs «SPEC_PATH_OR_SECTION_LIST»
2. Placeholder scan (no TBD/TODO/similar-to-N)
3. Consistency of names/paths/model ids across tasks
4. «EXTRA_REVIEW_CHECKS»

## Tool surface you may reference (do not re-implement)

«DOCUMENT_ASSUMED_APIS — e.g. cursor_run model allow-list, requireNonClaude, cursor_answer, NEEDS_CONTEXT»

Begin now. Read only what you need to name exact paths, then write the full plan.
```

### Example: filled brief (driving use case)

Orchestrator has an approved design at
`docs/superpowers/specs/2026-07-09-cursor-delegate-model-layer-design.md` §8
and wants Grok to author the skill+catalog plan. Filled prompt body (abbreviated
placeholders shown filled):

```text
You are a plan-writing delegate. The orchestrator (a different model) already
brainstormed and approved the design. Your job is to write ONE detailed
implementation plan markdown file — planning document only.

## Hard rules

- Do NOT implement code. Do NOT modify source. Do NOT run build/test commands
  except read-only inspection needed to name exact paths (`ls`, `rg`, `Read`).
- Do NOT commit. Do NOT create git commits or PRs.
- Your only deliverable is the plan content (and, if the orchestrator asked you
  to write a file, that single markdown path).
- If you lack a fact that blocks a correct plan (missing path, unclear API,
  ambiguous requirement), stop and ask. End your message with the question as
  the body and a trailing line exactly:

  STATUS: NEEDS_CONTEXT

  The orchestrator will answer via cursor_answer and you will resume. Do not
  guess through blockers.
- When the plan is complete, end with:

  STATUS: DONE

## Output path

Write the plan to: docs/superpowers/plans/2026-07-09-delegate-skill.md

## Plan document header (required)

Start the plan with this header shape (fill Goal / Architecture / Tech Stack):

# Delegate Skill & Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Claude Code delegate skill and rewrite config/agents/catalog.md around model + requireNonClaude.

**Architecture:** Documentation-only skill + catalog; verification via rg/file checks.

**Tech Stack:** Claude Code plugin skills, markdown catalog, existing MCP tools.

## Global Constraints

- Scope only design-spec §8 (skill + catalog). Model layer, needs-input, and doctor are already implemented — document them, do not build them.
- Allow-list ids: composer-2.5, cursor-grok-4.6-xhigh, cursor-grok-4.6-high, cursor-grok-4.5-high, gemini-3.5-flash, gpt-5.6-sol-high, gpt-5.6-terra-high.
- Model picks: composer-2.5 bulk; cursor-grok-4.6-xhigh plan-writing/coding; gemini-3.5-flash / gpt-5.6-sol-high / gpt-5.6-terra-high diverse review with requireNonClaude: true.
- Catalog: map roles to model + requireNonClaude columns; keep governing principle.
- No unit tests; use verification steps. Commit after each task.

## Spec / context (authoritative)

Paste §1 (driving use case) and §8 from
docs/superpowers/specs/2026-07-09-cursor-delegate-model-layer-design.md, plus
current config/agents/catalog.md and plugin/plugin.json layout notes.

## Assumed already true

- Model allow-list + requireNonClaude on cursor_run
- cursor_answer(jobId, answer) + NEEDS_CONTEXT parked jobs with uniform jobId

## Out of scope

- Implementing src/* model resolver, doctor tool, or needs-input runtime

## File structure (target)

| Path | Role |
|------|------|
| skills/delegate/SKILL.md | Orchestration playbook (single live doc) |
| config/agents/catalog.md | Roles → model + requireNonClaude |

## Methodology for THIS plan

Documentation-only: verification steps (file existence, rg/grep), not unit tests.

## Task structure (every task)

(Use the Task N shape from the template. Show full markdown to write in each step.)

## No placeholders in the plan you write

Never leave TBD / TODO / similar-to-N.

## Self-Review (end of your plan)

1. Spec coverage vs §8
2. Placeholder scan
3. Catalog model ids match the allow-list
4. Model ids match the allow-list

## Tool surface you may reference (do not re-implement)

cursor_run(model, requireNonClaude, …); cursor_answer(jobId, answer);
NEEDS_CONTEXT parked job always carries jobId.

Begin now. Read only what you need to name exact paths, then write the full plan.
```

### Example `cursor_run` wrapper

```json
{
  "model": "cursor-grok-4.6-xhigh",
  "capability": "ask",
  "prompt": "<paste filled PLAN-WRITER BRIEF TEMPLATE here>"
}
```

If the result is `NEEDS_CONTEXT`, call:

```json
{
  "jobId": "<parked job id>",
  "answer": "<orchestrator answer>"
}
```

on `cursor_answer`, then continue until terminal status.

## Review after plan-writing

Prefer a catalog **Design-critic** or **Verifier** on `gemini-3.5-flash` or
`gpt-5.6-sol-high` with `requireNonClaude: true` — never `cursor-grok-4.6-xhigh` reviewing
its own plan.
