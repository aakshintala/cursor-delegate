---
name: delegate
description: >
  Delegate coding, research, plan-writing, or review work to other models through the
  `delegate` CLI. Use for fanning work out, picking a model, writing a brief, waiting on
  or reading a job, or answering a NEEDS_CONTEXT job with resume.
---

# Delegate

You are the **orchestrator**. Run `delegate` (no arguments) for the command syntax, and
`delegate models` for the live model table. Never review an agent's output with the model that
produced it.

Only `cursor` and `pi` models run today. `claude` models appear in `delegate models`, but `run`
rejects them with "backend not implemented" (exit 2). A `pi` model with `--capability read-only`
is rejected (`pi cannot enforce read-only`, exit 2): pi runs need `read-write`.

## When to delegate

Delegate token-heavy plan writing (you hold the approved spec), mechanical or well-scoped
coding, uncorrelated review, and bulk triage or summarising. Keep the product decision, and any
edit the user wants to watch, for yourself.

Size is no reason to skip it. A three-line edit goes through `delegate run --gate` too: the
gate is what you are buying, and a hand edit skips it.

## Model picks

| Use | Model |
|---|---|
| Bulk, default | `composer-2.5` |
| Hard work, plan writing | `grok-4.7-high`, `grok-4.7-xhigh` |
| Cheap bulk (pi) | `opencode-go/muse-spark-1.3-contributor` |
| Moderate | `claude-sonnet-5-5`, `claude-opus-5-5` (claude, not yet runnable) |
| Escalation only | `openai-codex/*` (pi) |

## Roles

Each role is a `delegate run` you construct; nothing is configured anywhere.

| Role | Model | `--capability` | Prompt shape |
|---|---|---|---|
| Verifier | `grok-4.7-xhigh` | `read-only` | try to refute the claim, find the bug |
| Triager | `composer-2.5` | `read-only` | classify, route, summarise |
| Design-critic | `grok-4.7-high` | `read-only` | critique a design, surface risks |
| Codemod | `composer-2.5` | `read-write` | mechanical edit across a tree |
| Re-implementer | `grok-4.7-xhigh` | `read-write` | rebuild a component from a spec |
| Implementer | `composer-2.5` | `read-write` | one planned task |
| Spec / quality reviewer | not the implementer's model | `read-only` | review for gaps / quality |

## Running

```bash
delegate run --model composer-2.5 --capability read-write --cwd /abs/repo \
  --gate 'cargo fmt --check && cargo clippy -- -D warnings && cargo test' < brief.md
```

`run` prints the job id and returns; the job runs in a detached supervisor. A `read-write` job
holds an exclusive lock on its cwd, so a second one there exits 3 (BUSY).

**Write the gate as a shell command** that `/bin/sh -c` runs: an English postcondition is a
syntax error, and nothing is checked. Make it the next consumer's first action: if a packer
feeds a runner, the gate runs the runner. A gate that greps for a file is not a gate. Read
`result.gateResult` in the record, and rerun the check yourself when `passed` is false.

`delegate resume <id>` takes the new prompt on stdin and prints the new job id. `--model` must
stay on the same backend. It adds `supersededBy` to the old record, and exits 2 on a `RUNNING`
job or a record with no session id (a `CANCELLED` one). Answer `NEEDS_CONTEXT` with it.

`--tool-idle-ms` widens how long a running tool may stay silent before the idle watchdog
kills the job (default 1800000, 30 min; a model silent between tools gets 300000). It also
bounds the gate.

## Waiting

Run this with `run_in_background: true`; it is the only way to wait. One notification arrives
when every listed job is terminal, with their records on stdout:

```bash
delegate watch <id-a> <id-b> --timeout 1800
```

## Reading the result

The record is `$TMPDIR/delegate-jobs/<id>.json` (a terminal record has no `lastHeartbeatAt`):

```json
{"status":"DONE","result":{"status":"DONE","text":"Renamed the helper in 4 files.\n\nSTATUS: DONE","sessionId":"7c1e0a52-3b9f-4e0c-9a55-0d6f1b2c8e41","backend":"cursor","model":"composer-2.5","usage":{"inputTokens":18234,"outputTokens":1207,"cacheReadTokens":9100,"cacheWriteTokens":0},"costUsd":0.0148,"costEstimated":true,"durationMs":84213,"jobId":"d2f4a8e6-51c7-4b3a-8f90-6e1a7c3b5d02"},"supervisorPid":48213,"resume":{"model":"composer-2.5","cwd":"/abs/repo","capability":"read-write","sessionId":"7c1e0a52-3b9f-4e0c-9a55-0d6f1b2c8e41","gate":"cargo test","toolIdleMs":null}}
```

`status` is `DONE`, `DONE_WITH_CONCERNS`, `BLOCKED`, `NEEDS_CONTEXT`, `ERROR` (from the
agent's trailing STATUS line or the supervisor), `CANCELLED`, or `STALLED` (the idle watchdog
killed it: rerun, with a larger `--tool-idle-ms` if it stalled inside a tool). `text` is the agent's final message; for
`NEEDS_CONTEXT` it is the question.

- `result.gateResult`: a failing gate downgrades `DONE` to `DONE_WITH_CONCERNS`; it holds the
  gate's `exitCode` and `outputTail`.
- `result.changeSet`: git delta for the cwd (`newCommits`, `filesChanged`, `diffstat`,
  `uncommittedFiles`).
- `result.concerns`: warnings from the CLI, e.g. commits landed but the tree is still dirty.

- **Resume chain:** follow `supersededBy` to the newest record; the old one stays as it was.
- **Stuck job:** `watch` rewrites a record whose supervisor pid is dead to `ERROR` ("supervisor
  died"). A live supervisor whose `lastHeartbeatAt` has gone stale is not healed: `delegate
  cancel` it and rerun.
- A weak model may skip the STATUS line and guess; your review of the artifact is the backstop.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | ok |
| 1 | `watch` timed out, or `doctor` hard failure |
| 2 | bad input or unknown job |
| 3 | BUSY: a read-write job already holds that cwd |

## Briefs

Every brief says "Do not delegate further." and states its goal, the files in scope, and the
check that proves it done.

**Verify your own brief before sending it.** Reviewing the diff cannot catch an error you
wrote, because the diff matches the brief. Before writing "every caller passes X", run the
search and read the surviving branch. Tell the delegate to confirm stated premises before
acting on them.

**When the report contradicts a premise you supplied, believe the delegate first.** Get
evidence before overriding it: search for the symbol's callers, or `git log --diff-filter=D`
for a vanished file. Reading the diff is not evidence.

## Plan writing

1. Fill every placeholder in `plan-writer-brief.md`; never send an unfilled template.
2. `delegate run --model grok-4.7-xhigh --capability read-only < brief.md`, then `watch`. Use
   `read-write` only when the delegate must land the plan in the repo.
3. Answer `NEEDS_CONTEXT` with `delegate resume`.
4. Review the plan yourself, then run a Verifier or Design-critic on a different model.
