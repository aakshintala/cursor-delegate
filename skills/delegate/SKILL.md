---
name: delegate
description: >
  Run work on other models through the `delegate` CLI. Use when starting a delegated job,
  picking a model, writing a brief, waiting on or reading a job, or answering a NEEDS_CONTEXT
  job with resume.
---

# Delegate

Run `delegate` (no arguments) for the command syntax, and `delegate models` for the live model
table with backends and prices.

## Model tiers

Pick from the tier that fits the work, in this order of preference:

| Tier | Fits | Order |
|---|---|---|
| Read | exploration, fact-finding, research, triage | `composer-2.5` → `openai-codex/gpt-6-luna:medium` |
| Work | most implementation and review | `opencode-go/muse-spark-1.3-contributor` → `claude-sonnet-5-5` → `openai-codex/gpt-6-luna:xhigh` |
| Hard | hard work, and escalation after a Work model fails | `grok-4.7-xhigh` → `claude-opus-5-5` → `openai-codex/gpt-6.1-sol` |

A provider's **headroom** is its remaining quota on its tightest window (session, weekly or
monthly). The providers are cursor (`composer-2.5`, `grok-*`), opencode-go (`opencode-go/*`),
Anthropic (`claude-*`) and OpenAI (`openai-codex/*`).

- **Read** picks itself: the first model in the order whose provider has at least 20% headroom.
- **Work and Hard:** once per session, propose one pick per tier to the owner with each
  provider's headroom, and use the confirmed picks for the rest of the session. Ask again only
  when quota shifts. When the owner is unavailable, use the last confirmed picks, step down the
  tier order when a provider runs low, and report the switch.
- Review an agent's output with a different model from the one that produced it.

## Briefs

Every brief carries exactly one of these lines, verbatim:

- Worker: "Do not delegate further."
- Sub-orchestrator: "You may delegate through `delegate`; workers you start must not delegate
  further."

A brief without one is incomplete.

## Running

```bash
delegate run --model composer-2.5 --cwd /abs/repo \
  --gate 'cargo fmt --check && cargo clippy -- -D warnings && cargo test' < brief.md
```

`run` prints the job id and returns; the job runs in a detached supervisor.
Every job can write, so a read task says "do not edit" in its brief, the record's
`changeSet` shows any write, and parallel jobs need separate cwds (one worktree per lane).

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

`watch` blocks until every listed job is terminal, then prints their records on stdout. It is
the only way to wait; run it in the background the way your harness's instructions say.

```bash
delegate watch <id-a> <id-b> --timeout 1800
```

## Reading the result

The record is `$TMPDIR/delegate-jobs/<id>.json` (a terminal record has no `lastHeartbeatAt`):

```json
{"status":"DONE","result":{"status":"DONE","text":"Renamed the helper in 4 files.\n\nSTATUS: DONE","sessionId":"7c1e0a52-3b9f-4e0c-9a55-0d6f1b2c8e41","backend":"cursor","model":"composer-2.5","usage":{"inputTokens":18234,"outputTokens":1207,"cacheReadTokens":9100,"cacheWriteTokens":0},"costUsd":0.0148,"costEstimated":true,"durationMs":84213,"jobId":"d2f4a8e6-51c7-4b3a-8f90-6e1a7c3b5d02"},"supervisorPid":48213,"resume":{"model":"composer-2.5","cwd":"/abs/repo","sessionId":"7c1e0a52-3b9f-4e0c-9a55-0d6f1b2c8e41","gate":"cargo test","toolIdleMs":null}}
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
