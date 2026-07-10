# Delegate Skill & Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Claude Code `delegate` skill (orchestration playbook + plan-writer brief) and rewrite `config/agents/catalog.md` so roles map to allow-list `model` ids + `requireNonClaude` instead of deleted tiers.

**Architecture:** Documentation-only subsystem. `plugin/skills/delegate/SKILL.md` is the lean, model-invoked playbook (when to delegate, model picks, needs-input resume). `plugin/skills/delegate/reference.md` holds the reusable PLAN-WRITER BRIEF TEMPLATE and expanded call examples. `config/agents/catalog.md` remains a convention layer over `cursor_run` — same agent rows, new columns. No runtime code, no unit tests; verify with file existence and `rg`/`grep` assertions.

**Tech Stack:** Claude Code plugin skills (YAML frontmatter + markdown under `plugin/skills/`), existing MCP tool surface (`cursor_run`, `cursor_answer`, job tools), markdown catalog at `config/agents/catalog.md`.

## Global Constraints

- Scope is **only** design-spec §8: create `plugin/skills/delegate/SKILL.md` (+ `reference.md`) and rewrite `config/agents/catalog.md`. Do **not** implement or re-plan model layer (§4–5), needs-input (§6), or doctor (§7).
- Assume model-layer and needs-input are **already implemented**. Document that tool surface; do not build it.
- Allow-list model ids (exact strings): `composer-2.5` (default), `grok-4.5-xhigh`, `gemini-3.5-flash`, `gpt-5.5-high`.
- Model picks in the skill: `composer-2.5` for bulk; `grok-4.5-xhigh` for plan-writing/coding; `gemini-3.5-flash` / `gpt-5.5-high` for diverse review with `requireNonClaude: true`.
- Catalog rewrite: replace the `tier` column with `model` + `requireNonClaude`. After rewrite, `config/agents/catalog.md` must contain **zero** matches for `tier` (case-insensitive word use as a column/param — no `tier` column, no `diversity`/`cheap-bulk`/`standard`/`coding-specialist` tier names).
- Governing principle stays verbatim: *never review an agent's output with the same model that produced it.* It is now backed by `requireNonClaude` (and distinct recommended models), not the deleted `diversity` tier.
- Skill conventions: `SKILL.md` has YAML frontmatter with `name` and `description`; body is the playbook. Progressive disclosure: put the long PLAN-WRITER BRIEF TEMPLATE in `reference.md` and link it from `SKILL.md`.
- This subsystem is documentation — **no unit tests, no `npm test` / `npm run build`**. Every content step ends with a verification command (`test -f`, `rg`, `grep`) and an expected outcome.
- Commit after each task. Do not push.

## File structure (target)

| Path | Role |
|------|------|
| `plugin/skills/delegate/SKILL.md` | Model-invoked orchestration playbook |
| `plugin/skills/delegate/reference.md` | PLAN-WRITER BRIEF TEMPLATE + expanded examples |
| `config/agents/catalog.md` | Convention tuples: roles → `model` + `requireNonClaude` |

**Assumed already true (do not re-implement):**

- `config/models.json` allow-list with the four ids above; default `composer-2.5`.
- `cursor_run` accepts `model?: string` and `requireNonClaude?: boolean` (no `tier` param).
- `cursor_answer(jobId, answer)` resumes a parked `NEEDS_CONTEXT` job via `--resume <sessionId>`.
- Any run ending in `NEEDS_CONTEXT` retains a `jobId` (parked job) even if it never detached for slowness.
- Delegate final message text *is* the question when status is `NEEDS_CONTEXT`.
- Plugin layout remains `plugin/plugin.json` + `plugin/.mcp.json`; skills under `plugin/skills/<name>/SKILL.md` are auto-discovered (no `plugin.json` skills field required).

**Documented tool surface (for skill/catalog authors — already live):**

| Tool | Role in this playbook |
|------|------------------------|
| `cursor_run` | Start a delegation (`prompt`, optional `model`, `requireNonClaude`, `capability`, `isolation`, …) |
| `cursor_answer` | Resume a parked job: `{ jobId, answer }` |
| `cursor_poll` / `cursor_wait` / `cursor_wait_any` / `cursor_wait_all` / `cursor_cancel` | Async job control when `background: true` or deadline detach |

---

### Task 1: Create `plugin/skills/delegate/SKILL.md`

**Files:**
- Create: `plugin/skills/delegate/SKILL.md`

**Interfaces:**
- Consumes: assumed tool surface (`cursor_run`, `cursor_answer`, allow-list ids, `requireNonClaude`, `NEEDS_CONTEXT` parked-job flow)
- Produces: skill frontmatter `name: delegate`; body sections that Task 2's `reference.md` is linked from (`## Plan-writer brief` pointer); model-pick table that Task 3's catalog must stay consistent with

- [ ] **Step 1: Create the skill directory**

```bash
mkdir -p plugin/skills/delegate
```

- [ ] **Step 2: Verify the directory exists**

Run: `test -d plugin/skills/delegate && echo OK`

Expected: `OK`

- [ ] **Step 3: Write `plugin/skills/delegate/SKILL.md`**

Write this exact file:

```markdown
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
  to `grok-4.5-xhigh`.
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
| Plan-writing / strong coding | `grok-4.5-xhigh` | `false` (omit) |
| Diverse review (uncorrelated) | `gemini-3.5-flash` or `gpt-5.5-high` | `true` |

Omit `model` only when `composer-2.5` is acceptable — that is the server default.

For reusable role tuples (Verifier, Triager, …), see
`config/agents/catalog.md` in the cursor-delegate repo (or the installed plugin's
bundled catalog). Match the catalog's `model` + `requireNonClaude` columns.

## Calling `cursor_run`

Minimum shape:

```json
{
  "prompt": "<task for the Cursor agent>",
  "model": "grok-4.5-xhigh",
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
3. `cursor_run` with `model: "grok-4.5-xhigh"`, `capability: "ask"` or `"plan"`
   (read-only plan authoring; use `write` only if the plan must be written into the
   repo by the delegate).
4. On `NEEDS_CONTEXT`, answer via `cursor_answer` and continue until `DONE` /
   `DONE_WITH_CONCERNS` / `BLOCKED` / `ERROR`.
5. Review the plan yourself. For a second opinion, run a Verifier or Design-critic
   catalog role on a **different** model with `requireNonClaude: true`.

## Plan-writer brief

Before any plan-writing `cursor_run`, read and apply
[reference.md](./reference.md) (PLAN-WRITER BRIEF TEMPLATE + example filled brief).
```

- [ ] **Step 4: Verify frontmatter keys and required section headings**

Run:

```bash
test -f plugin/skills/delegate/SKILL.md && echo FILE_OK
rg -n '^---$' plugin/skills/delegate/SKILL.md | head -n 2
rg -n '^name: delegate$' plugin/skills/delegate/SKILL.md
rg -n '^description:' plugin/skills/delegate/SKILL.md
rg -n '^## When to delegate$' plugin/skills/delegate/SKILL.md
rg -n '^## Model picks$' plugin/skills/delegate/SKILL.md
rg -n '^## Calling `cursor_run`$' plugin/skills/delegate/SKILL.md
rg -n '^## Needs-input resume flow$' plugin/skills/delegate/SKILL.md
rg -n '^## Driving use case: delegated plan-writing$' plugin/skills/delegate/SKILL.md
rg -n '^## Plan-writer brief$' plugin/skills/delegate/SKILL.md
rg -n 'composer-2\.5' plugin/skills/delegate/SKILL.md
rg -n 'grok-4\.5-xhigh' plugin/skills/delegate/SKILL.md
rg -n 'gemini-3\.5-flash' plugin/skills/delegate/SKILL.md
rg -n 'gpt-5\.5-high' plugin/skills/delegate/SKILL.md
rg -n 'requireNonClaude' plugin/skills/delegate/SKILL.md
rg -n 'cursor_answer' plugin/skills/delegate/SKILL.md
rg -n 'NEEDS_CONTEXT' plugin/skills/delegate/SKILL.md
rg -n 'reference\.md' plugin/skills/delegate/SKILL.md
```

Expected:
- `FILE_OK`
- Two `---` frontmatter delimiters near the top
- Matches for `name: delegate`, `description:`, every `##` heading listed above
- Matches for all four model ids, `requireNonClaude`, `cursor_answer`, `NEEDS_CONTEXT`, and `reference.md`

- [ ] **Step 5: Verify no tier vocabulary in the skill**

Run: `rg -n -i 'tier|cheap-bulk|coding-specialist|\bdiversity\b' plugin/skills/delegate/SKILL.md; echo EXIT:$?`

Expected: no matching lines; `EXIT:1` (rg exit code 1 = no matches). If any match appears, remove it before committing.

- [ ] **Step 6: Commit**

```bash
git add plugin/skills/delegate/SKILL.md
git commit -m "$(cat <<'EOF'
docs(skill): add cursor-delegate orchestration playbook

EOF
)"
```

---

### Task 2: Create `plugin/skills/delegate/reference.md`

**Files:**
- Create: `plugin/skills/delegate/reference.md`

**Interfaces:**
- Consumes: Task 1's `SKILL.md` pointer (`## Plan-writer brief` → `./reference.md`); writing-plans methodology (header, Global Constraints, bite-sized tasks, verification-adapted for delegates, Self-Review); assumed `NEEDS_CONTEXT` / `cursor_answer` surface
- Produces: the canonical PLAN-WRITER BRIEF TEMPLATE string that orchestrators paste into `cursor_run.prompt` after filling placeholders

- [ ] **Step 1: Write `plugin/skills/delegate/reference.md`**

Write this exact file:

```markdown
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
- Catalog: replace tier column with model + requireNonClaude; keep governing principle.
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
3. No lingering tier references in catalog
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
```

- [ ] **Step 2: Verify reference file contents**

Run:

```bash
test -f plugin/skills/delegate/reference.md && echo FILE_OK
rg -n '^# Delegate skill — reference$' plugin/skills/delegate/reference.md
rg -n '^## PLAN-WRITER BRIEF TEMPLATE$' plugin/skills/delegate/reference.md
rg -n 'STATUS: NEEDS_CONTEXT' plugin/skills/delegate/reference.md
rg -n 'STATUS: DONE' plugin/skills/delegate/reference.md
rg -n 'REQUIRED SUB-SKILL' plugin/skills/delegate/reference.md
rg -n 'Global Constraints' plugin/skills/delegate/reference.md
rg -n 'Self-Review' plugin/skills/delegate/reference.md
rg -n 'grok-4\.5-xhigh' plugin/skills/delegate/reference.md
rg -n 'cursor_answer' plugin/skills/delegate/reference.md
rg -n '^## Example: filled brief \(driving use case\)$' plugin/skills/delegate/reference.md
rg -n '^## Example `cursor_run` wrapper$' plugin/skills/delegate/reference.md
rg -n '^## Review after plan-writing$' plugin/skills/delegate/reference.md
```

Expected: `FILE_OK` and a match line for each pattern.

- [ ] **Step 3: Verify SKILL.md still points at reference.md**

Run: `rg -n '\[reference\.md\]\(\./reference\.md\)' plugin/skills/delegate/SKILL.md`

Expected: at least one match (Plan-writer brief section).

- [ ] **Step 4: Verify no tier vocabulary in reference.md**

Run: `rg -n -i 'tier|cheap-bulk|coding-specialist|\bdiversity\b' plugin/skills/delegate/reference.md; echo EXIT:$?`

Expected: no matching lines; `EXIT:1`.

- [ ] **Step 5: Commit**

```bash
git add plugin/skills/delegate/reference.md
git commit -m "$(cat <<'EOF'
docs(skill): add plan-writer brief template reference

EOF
)"
```

---

### Task 3: Rewrite `config/agents/catalog.md`

**Files:**
- Modify: `config/agents/catalog.md` (full-file replace)

**Interfaces:**
- Consumes: Task 1 model-pick table; allow-list ids; governing principle from current catalog / spec §8
- Produces: catalog rows that orchestrators and the skill treat as the role→`cursor_run` convention (columns: Agent, model, requireNonClaude, capability, isolation, prompt shape)

- [ ] **Step 1: Replace `config/agents/catalog.md` with the model-based catalog**

Write this exact file (full replace):

```markdown
# Agent catalog (convention layer)

These "agents" are **convention tuples over `cursor_run`** — the host agent (the controller)
constructs each call. Nothing here is Cursor-side configuration; this is documentation, not code.

**Governing principle:** *never review an agent's output with the same model that produced it.*
For reviewer roles, set `requireNonClaude: true` and pick a non-Claude allow-list model so the
second opinion stays uncorrelated.

| Agent | model | requireNonClaude | capability | isolation | prompt shape |
|---|---|---|---|---|---|
| **Verifier** | `gpt-5.5-high` | `true` | `ask` | None | adversarial: *try to refute the claim / find the bug* |
| **Triager** | `composer-2.5` | `false` | `ask` | None | classify / route / summarize an issue |
| **Design-critic** | `gemini-3.5-flash` | `true` | `plan` | None | critique a design, surface risks, propose alternatives |
| **Codemod** | `composer-2.5` | `false` | `write` | CallerProvided | mechanical, well-scoped edit across a tree |
| **Re-implementer** | `grok-4.5-xhigh` | `false` | `write` | CallerProvided | rebuild a component from a spec |
| **SP-implementer** | `composer-2.5` | `false` | `write` | CallerProvided | implement a single planned task |
| **SP spec-reviewer** | `gemini-3.5-flash` | `true` | `ask` | None | review a spec for gaps (different model than implementer) |
| **SP quality-reviewer** | `gpt-5.5-high` | `true` | `ask` | None | review an implementation for quality (different model than implementer) |

Notes:
- Implementers use `CallerProvided` isolation so writes land in a known working tree and participate
  in the same-path write lock.
- Always pass `verifyCommands` to bound what an implementer may run, and a `gate` to enforce the
  postcondition the tool itself checks.
- Reviewers (Verifier, Design-critic, SP spec-reviewer, SP quality-reviewer) use a **different**
  allow-list model from the producer and pass `requireNonClaude: true`.
- Plan-writing (not a named row above) uses `grok-4.5-xhigh` with `capability` `ask` or `plan`;
  see `plugin/skills/delegate/SKILL.md` and the PLAN-WRITER BRIEF TEMPLATE in
  `plugin/skills/delegate/reference.md`.
- Allow-list ids only: `composer-2.5`, `grok-4.5-xhigh`, `gemini-3.5-flash`, `gpt-5.5-high`.
  Omit `model` only when the default `composer-2.5` is intended.
```

- [ ] **Step 2: Verify new columns and governing principle**

Run:

```bash
test -f config/agents/catalog.md && echo FILE_OK
rg -n 'Governing principle' config/agents/catalog.md
rg -n 'never review an agent.s output with the same model that produced it' config/agents/catalog.md
rg -n '^\| Agent \| model \| requireNonClaude \| capability \| isolation \| prompt shape \|$' config/agents/catalog.md
rg -n 'composer-2\.5' config/agents/catalog.md
rg -n 'grok-4\.5-xhigh' config/agents/catalog.md
rg -n 'gemini-3\.5-flash' config/agents/catalog.md
rg -n 'gpt-5\.5-high' config/agents/catalog.md
rg -n '\| \*\*Verifier\*\* \| `gpt-5\.5-high` \| `true`' config/agents/catalog.md
rg -n '\| \*\*Triager\*\* \| `composer-2\.5` \| `false`' config/agents/catalog.md
rg -n '\| \*\*Design-critic\*\* \| `gemini-3\.5-flash` \| `true`' config/agents/catalog.md
rg -n '\| \*\*Codemod\*\* \| `composer-2\.5` \| `false`' config/agents/catalog.md
rg -n '\| \*\*Re-implementer\*\* \| `grok-4\.5-xhigh` \| `false`' config/agents/catalog.md
rg -n '\| \*\*SP-implementer\*\* \| `composer-2\.5` \| `false`' config/agents/catalog.md
rg -n '\| \*\*SP spec-reviewer\*\* \| `gemini-3\.5-flash` \| `true`' config/agents/catalog.md
rg -n '\| \*\*SP quality-reviewer\*\* \| `gpt-5\.5-high` \| `true`' config/agents/catalog.md
rg -n 'plugin/skills/delegate' config/agents/catalog.md
```

Expected: `FILE_OK` and a match for each pattern (governing principle line may use a curly/straight apostrophe — if the apostrophe pattern fails, confirm the verbatim principle sentence is present with `rg -n 'never review' config/agents/catalog.md`).

- [ ] **Step 3: Assert no lingering tier references**

Run:

```bash
rg -n -i 'tier|cheap-bulk|coding-specialist|\bdiversity\b|standard`' config/agents/catalog.md; echo EXIT:$?
rg -n '\| tier \|' config/agents/catalog.md; echo TIER_COL:$?
```

Expected:
- First command: no matches; `EXIT:1`
- Second command: no matches; `TIER_COL:1`

Note: the word `standard` must not appear as a tier name. If you need the English word "standard" in prose, avoid it in this file — use "convention" / "default" instead (the file above does not use it).

- [ ] **Step 4: Assert every catalog model is on the allow-list**

Run:

```bash
rg -o '`[^`]+`' config/agents/catalog.md | tr -d '`' | sort -u
```

Expected unique backtick tokens include at least:
`composer-2.5`, `grok-4.5-xhigh`, `gemini-3.5-flash`, `gpt-5.5-high`, `true`, `false`, `ask`, `plan`, `write`, `CallerProvided`
and must **not** include any model id outside the allow-list (no `gpt-5.5-medium`, no `grok-4.5-medium`, no Claude ids).

- [ ] **Step 5: Commit**

```bash
git add config/agents/catalog.md
git commit -m "$(cat <<'EOF'
docs(catalog): map agent roles to model and requireNonClaude

EOF
)"
```

---

### Task 4: Cross-file verification (skill ↔ catalog ↔ allow-list)

**Files:**
- Modify: none (verification only; fix drift in the three docs if any check fails)

**Interfaces:**
- Consumes: outputs of Tasks 1–3
- Produces: confirmed consistency before the subsystem is considered done

- [ ] **Step 1: Confirm skill tree layout**

Run:

```bash
find plugin/skills/delegate -type f | sort
```

Expected:

```
plugin/skills/delegate/SKILL.md
plugin/skills/delegate/reference.md
```

- [ ] **Step 2: Confirm all four allow-list ids appear in both skill and catalog**

Run:

```bash
for id in composer-2.5 grok-4.5-xhigh gemini-3.5-flash gpt-5.5-high; do
  echo "== $id =="
  rg -n "$id" plugin/skills/delegate/SKILL.md plugin/skills/delegate/reference.md config/agents/catalog.md
done
```

Expected: each id appears in `SKILL.md` and `catalog.md`. `reference.md` must include at least `grok-4.5-xhigh` (plan-writer default); other ids may appear in the review section (`gemini-3.5-flash` / `gpt-5.5-high`).

- [ ] **Step 3: Confirm reviewer roles require non-Claude in catalog and skill guidance matches**

Run:

```bash
rg -n 'requireNonClaude: true' plugin/skills/delegate/SKILL.md
rg -n '\| `true` \| `ask`|\| `true` \| `plan`' config/agents/catalog.md
rg -n 'Verifier|Design-critic|SP spec-reviewer|SP quality-reviewer' config/agents/catalog.md
```

Expected:
- Skill documents `requireNonClaude: true` for diverse review
- Catalog rows for Verifier, Design-critic, SP spec-reviewer, SP quality-reviewer use `` `true` ``

- [ ] **Step 4: Repo-wide tier sweep for this subsystem's files**

Run:

```bash
rg -n -i 'tier|cheap-bulk|coding-specialist|\bdiversity\b' \
  plugin/skills/delegate/SKILL.md \
  plugin/skills/delegate/reference.md \
  config/agents/catalog.md \
  ; echo EXIT:$?
```

Expected: no matches; `EXIT:1`.

- [ ] **Step 5: Commit only if Step 1–4 forced doc fixes; otherwise skip**

If you had to edit any of the three files to pass Steps 1–4:

```bash
git add plugin/skills/delegate/SKILL.md plugin/skills/delegate/reference.md config/agents/catalog.md
git commit -m "$(cat <<'EOF'
docs: align delegate skill and catalog model ids

EOF
)"
```

If all checks passed with no edits, print `NO_COMMIT_NEEDED` and continue.

---

## Self-Review

**1. Spec coverage (§8):**
- `plugin/skills/delegate/SKILL.md` (+ reference) — Tasks 1–2: when to delegate, model picks (`composer-2.5` / `grok-4.5-xhigh` / `gemini-3.5-flash` / `gpt-5.5-high` + `requireNonClaude: true` for diverse review), plan/`NEEDS_CONTEXT`/`cursor_answer` resume flow, PLAN-WRITER BRIEF TEMPLATE (driving use case).
- `config/agents/catalog.md` rewrite — Task 3: `tier` → `model` + `requireNonClaude`; governing principle retained; Task 4 cross-checks.
- Explicitly excluded: model layer (§4–5), needs-input implementation (§6), doctor (§7) — documented as assumed, not planned.

**2. Placeholder scan:** Plan steps contain full file bodies (no TBD/TODO/"similar to Task N"). Verification commands include expected outcomes.

**3. No lingering tier references:** Tasks 1–4 each include `rg` assertions that skill + catalog contain no `tier` / old tier names.

**4. Model ids match allow-list:** Only `composer-2.5`, `grok-4.5-xhigh`, `gemini-3.5-flash`, `gpt-5.5-high` appear as model ids in the skill, reference, and catalog content prescribed above; Task 4 Step 2/3 re-checks.

**5. Documentation methodology:** No unit-test / build steps; verification is file existence + `rg`/`grep` only, matching the user's documentation-subsystem adaptation.
