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
  to `cursor-grok-4.5-high`.
- **Mechanical or well-scoped coding** (codemod, single planned task, rebuild from
  a clear spec).
- **Uncorrelated review** (refute a claim, critique a design, review a spec or
  implementation) on a **different** model than the producer.
- **Bulk triage / classify / summarize** where quality bar is lower than coding.

Do **not** delegate when you still need to invent the product decision yourself,
when the user must stay in the loop on every edit, or when the only available
models would review with the same id that produced the artifact.

## Model picks

Allow-list ids only (server rejects anything else):

| Intent | `model` | `requireNonClaude` |
|---|---|---|
| Bulk / cheap / default | `composer-2.5` | `false` (omit) |
| Plan-writing / strong coding | `cursor-grok-4.5-high` | `false` (omit) |
| Diverse review (uncorrelated) | `gemini-3.5-flash`, `gpt-5.6-sol-high`, or `gpt-5.6-terra-high` | `true` |

Omit `model` only when `composer-2.5` is acceptable — that is the server default.

For reusable role tuples (Verifier, Triager, …), see
`config/agents/catalog.md` in the cursor-delegate repo (or the installed plugin's
bundled catalog). Match the catalog's `model` + `requireNonClaude` columns.

## Calling `cursor_run`

Minimum shape:

```json
{
  "prompt": "<task for the Cursor agent>",
  "model": "cursor-grok-4.5-high",
  "capability": "ask"
}
```

Common additions:

- `capability`: `ask` | `plan` | `write` | `write-unsandboxed`
- `isolation`: `{ "type": "CallerProvided", "path": "<abs working tree>" }` for writes
- `verifyCommands`: string[] — only verify commands the agent may run
- `gate`: postcondition **you** (the tool) enforce after the agent
- `requireNonClaude`: `true` for reviewer roles
- `background`: `true` to fan out; then `cursor_wait` / `cursor_wait_any` / `cursor_wait_all`

Always end delegated prompts with an instruction to finish with a trailing
`STATUS: DONE` | `BLOCKED` | `NEEDS_CONTEXT` line (the server also injects a
status-convention block; reinforce it in plan-writer briefs).

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
2. Build the prompt from the **PLAN-WRITER BRIEF TEMPLATE** in
   [reference.md](./reference.md) — fill every placeholder; do not send an empty
   template.
3. `cursor_run` with `model: "cursor-grok-4.5-high"`, `capability: "ask"` or `"plan"`
   (read-only plan authoring; use `write` only if the plan must be written into the
   repo by the delegate).
4. On `NEEDS_CONTEXT`, answer via `cursor_answer` and continue until `DONE` /
   `DONE_WITH_CONCERNS` / `BLOCKED` / `ERROR`.
5. Review the plan yourself. For a second opinion, run a Verifier or Design-critic
   catalog role on a **different** model with `requireNonClaude: true`.

## Plan-writer brief

Before any plan-writing `cursor_run`, read and apply
[reference.md](./reference.md) (PLAN-WRITER BRIEF TEMPLATE + example filled brief).
