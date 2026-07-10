# Delegate skill — reference

Supporting material for [`SKILL.md`](./SKILL.md). Load this when building a
plan-writing brief or when you need a fully filled example.

## PLAN-WRITER BRIEF TEMPLATE

Copy everything inside the fence into `cursor_run.prompt` after replacing
placeholders (`«...»`). Do not leave placeholders unfilled. Use
`model: "grok-4.5-xhigh"` unless the user explicitly overrides with another
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

## Example: filled brief (driving use case)

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
- Allow-list ids: composer-2.5, grok-4.5-xhigh, gemini-3.5-flash, gpt-5.5-high.
- Model picks: composer-2.5 bulk; grok-4.5-xhigh plan-writing/coding; gemini-3.5-flash / gpt-5.5-high diverse review with requireNonClaude: true.
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
| plugin/skills/delegate/SKILL.md | Orchestration playbook |
| plugin/skills/delegate/reference.md | PLAN-WRITER BRIEF TEMPLATE |
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

## Example `cursor_run` wrapper

```json
{
  "model": "grok-4.5-xhigh",
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
`gpt-5.5-high` with `requireNonClaude: true` — never `grok-4.5-xhigh` reviewing
its own plan.
