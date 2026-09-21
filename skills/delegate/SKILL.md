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

**Size is not a reason to skip the delegate.** Once a session is set up to
implement through `cursor_run`, a three-line edit goes through it too. The round
trip is not what you are buying — the `gate` is, and a hand edit skips it. A
change that feels too trivial to brief means the brief will be short, not that
the delegate should be skipped.

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
  or `plan`; see [`plan-writer-brief.md`](./plan-writer-brief.md).

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
- `gate`: postcondition **you** (the tool) enforce after the agent — a shell
  command, see [Writing a gate](#writing-a-gate)
- `requireNonClaude`: `true` for reviewer roles
- `background`: `true` to fan out; then wait for completion (see below)

Always end delegated prompts with an instruction to finish with a trailing
`STATUS: DONE` | `BLOCKED` | `NEEDS_CONTEXT` line (the server also injects a
status-convention block; reinforce it in plan-writer briefs).

## Writing a gate

`gate` is a **shell command string**, executed verbatim by `/bin/sh -c`. An
English postcondition ("the test output contains no failure line") makes `sh` die
on a syntax error: the job returns `DONE_WITH_CONCERNS` with
`gateResult.passed: false`, and nothing was checked. The brief still *looks*
gated. Put the prose version in the prompt body and keep `gate` runnable:

```
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Add an explicit `grep` wherever a tool's exit code is known to lie about failure.

**Read `gateResult` in the job record, not the top-level status**, and re-run the
verification yourself whenever `passed` is false.

**Write the gate as the next consumer's first action, not as an existence
check.** "The artifact was produced" and "the artifact works" come apart exactly
where permissions, encodings and platforms differ, which is where the bugs are.
If a packer feeds a runner, the gate runs the runner. If a generator feeds a
parser, the gate parses. A gate that greps for a file is not a gate.

## Waiting on jobs

**Under a minute:** block the turn with `cursor_wait` (one job), `cursor_wait_any`
(first of several) or `cursor_wait_all` (all of several). Each takes `timeoutMs`
(default 120000, clamped `[1000, 600000]`) and returns the current `RUNNING`
snapshot on timeout rather than hanging.

**Longer:** do not hold the turn inside `cursor_wait*`. Watch the status record
the server writes to disk, from a background shell, so the turn can end and one
notification arrives when the job or batch is done.

### The status record

Every dispatched job gets one JSON file at
`join(os.tmpdir(), "cursor-delegate-jobs", "<jobId>.json")` — resolve it as
`${TMPDIR:-/tmp}/cursor-delegate-jobs/${JOB_ID}.json`.

It holds exactly what `cursor_poll` would return: `{"status": "RUNNING",
"lastHeartbeatAt": <server ms>, "progress": {...}}` while running, and
`{"status": "<terminal>", "result": <RunOutput>}` when done — the full payload,
not a status label.

The server writes at start, refreshes every 30s while running, and writes once
more at the terminal transition. **If `lastHeartbeatAt` stops advancing while the
record still says `RUNNING`, the server died mid-job**: redispatch rather than
wait.

### The watcher

One bounded loop handles one job or many — the `timeout` is what stops a missing
or never-written record hanging forever. `jq` is the one host dependency.

```bash
JOB_IDS=( "<id-a>" )   # one entry per background cursor_run
FILES=(); for id in "${JOB_IDS[@]}"; do FILES+=( "${TMPDIR:-/tmp}/cursor-delegate-jobs/${id}.json" ); done

command -v jq >/dev/null || { echo "jq required" >&2; exit 1; }

timeout 300 bash -c '
  FILES=("$@")
  until all=true; for f in "${FILES[@]}"; do
           jq -e ".status != \"RUNNING\"" "$f" >/dev/null 2>&1 || { all=false; break; }
         done; $all
  do sleep 2; done
  for f in "${FILES[@]}"; do echo "=== $f ==="; cat "$f"; echo; done
' _ "${FILES[@]}"
```

In Claude Code, run that with the **Bash** tool and `run_in_background: true`.
Its stdout is the terminal `PollResult` JSON for every job.

### `NEEDS_CONTEXT` ends the wait, not the job

When a delegate parks for input the record leaves `RUNNING` with
`status: "NEEDS_CONTEXT"` and the full `result`, so the watcher exits and prints
it. Answer via [`cursor_answer`](#needs-input-resume-flow), then wait again on the
resumed job.

`cursor_answer` resumes under a **new jobId** and stamps the parked record with
`"supersededBy": "<newJobId>"` — a forward pointer, not a status change. The
original record stays `NEEDS_CONTEXT` forever; follow the chain to the new id.

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

## Briefing and trusting the delegate

**Verify your own brief before sending it.** Reviewing the returned diff does not
catch an error you authored: the diff matches the brief, so it reads correct. For
any claim of the form "every caller passes X" or "the remaining value is Y", run
the search and read the surviving branch before writing it into the brief. Where
a conditional is resolved at build or compile time, check which branch
*production* takes rather than which looks like the default.

**Ask the delegate to verify stated premises** rather than take them on trust. A
brief that says "confirm this before acting on it" turns your own error into a
report instead of a silent wrong change.

**When a delegate's report contradicts a premise you supplied, believe the
delegate first.** Before overriding its judgement — or attributing a change to it
— get evidence. Reading the diff is not evidence, and neither is the fact that a
delegate happened to be running. Search for the symbol's callers outside the file
before disputing a retention: a stale-sounding name often means a rename is owed,
not a deletion. For a file that vanished, `git log --diff-filter=D` and the
delegate's own reported file list are evidence; concurrency is not. Ask.

## Driving use case: delegated plan-writing

1. You hold the approved spec (brainstorm done).
2. Read [`plan-writer-brief.md`](./plan-writer-brief.md) and fill every
   placeholder. Never send an unfilled template.
3. `cursor_run` with `model: "cursor-grok-4.6-xhigh"` and `capability: "ask"` or
   `"plan"`. Use `write` only when the delegate must land the plan in the repo.
4. On `NEEDS_CONTEXT`, answer via `cursor_answer` and continue to a terminal
   status.
5. Review the plan yourself, then run a Verifier or Design-critic on a different
   model with `requireNonClaude: true`.

## Review after plan-writing

Prefer a catalog **Design-critic** or **Verifier** on `gemini-3.5-flash` or
`gpt-5.6-sol-high` with `requireNonClaude: true` — never `cursor-grok-4.6-xhigh` reviewing
its own plan.
