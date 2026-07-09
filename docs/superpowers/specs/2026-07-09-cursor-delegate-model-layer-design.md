# Design: cursor-delegate model-layer redesign (Path A)

**Date:** 2026-07-09
**Status:** Approved for planning
**Supersedes:** `handoff-2026-07-09-grok-tiers-brainstorm.md` (decisions carried forward and corrected below)

## 1. Motivation

cursor-delegate is an MCP bridge: Claude Code (the orchestrator, an LLM) calls
`cursor_run` to delegate coding/research work to a Cursor model via the local
`cursor-agent` CLI (`--print --output-format stream-json`).

**Driving use case.** An orchestrator (Opus) brainstorms a spec, then wants each
*plan-writing* pass done by a cheaper model (Grok 4.5) instead of burning Opus
tokens on token-heavy detailed plans. Opus fires off one or more delegated
plan-writes, reviews the results, and answers any question the delegate raises.

This use case is served entirely by the existing one-shot stream-json transport
plus an improved model-selection layer. It does **not** require ACP (see §9): the
question-answerer is the orchestrator, not a live human, so any clarifying question
inherently round-trips through a tool boundary regardless of transport.

## 2. Problems with today's design

1. **Tiers are jank.** Four symbolic tiers (`cheap-bulk`, `standard`,
   `coding-specialist`, `diversity`) where two map to the same model
   (`composer-2.5`). They are aliases a capable LLM caller could inline.
2. **Grok doesn't fit tiers.** Grok has multiple effort ids and the id labels are
   *offset from their real effort* (verified via `cursor-agent --list-models`):
   `grok-4.5-medium` = "Grok 4.5 **Low**", `grok-4.5-high` = "Grok 4.5 **Medium**",
   `grok-4.5-xhigh` = "Grok 4.5". A free-form caller reasoning "use `-medium` for a
   mid task" silently gets the low tier. This is a booby trap.
3. **Free-form `model` has sharp edges.** `price-map.json` keys on exact ids; an
   unlisted id breaks cost reporting. (`price-map.json` already has an orphan bare
   `gpt-5.5` entry that is not a valid id.)
4. **The one valuable tier is `diversity`** — a named intent ("uncorrelated second
   opinion") backed by a machine-enforced non-Claude contract (`DiversityClaudeError`).
   That contract must survive the tier drop, not degrade to prose.

## 3. Decisions (locked)

1. **Drop tiers entirely.** No `tier` param, no `tier-map.json`, no `tierOverrides`.
   Hard cut, no back-compat shim (the plugin ships as a whole).
2. **Curated model allow-list** replaces both tier-map and price-map. Not tiers
   (no symbolic indirection) and not free-form (no unpriced/booby-trapped ids).
3. **`nonClaudeDiversity` is kept and enforced**, via a family tag, with **hard
   reject** on conflict (not silent swap).
4. **Needs-input round-trip becomes first-class**: reuse the existing
   `NEEDS_CONTEXT` status + a new `cursor_answer` tool. No transport change.
5. **Add a `doctor` tool.**
6. **Add a `delegate` skill** and rewrite `catalog.md` around roles→models.
7. **ACP is deferred** behind a concrete trigger (§9).

## 4. Model layer

### 4.1 `config/models.json` (new; replaces `tier-map.json` + `price-map.json`)

Single source of truth. Prices are `$/MTok`. `family` drives the non-Claude
contract. Prices are maintained by hand — **the CLI exposes no pricing** (verified:
`agent about`, `agent models`, and `--list-models` return ids/labels only).

```json
{
  "default": "composer-2.5",
  "models": {
    "composer-2.5":     { "label": "Composer 2.5",     "family": "composer", "price": { "input": 0.5, "output": 2.5, "cacheRead": 0.2,  "cacheWrite": 0 } },
    "grok-4.5-xhigh":   { "label": "Grok 4.5",          "family": "grok",     "price": { "input": 2,   "output": 6,  "cacheRead": 0.5,  "cacheWrite": 0 } },
    "gemini-3.5-flash": { "label": "Gemini 3.5 Flash",  "family": "gemini",   "price": { "input": 1.5, "output": 9,  "cacheRead": 0.15, "cacheWrite": 0 } },
    "gpt-5.5-high":     { "label": "GPT-5.5 1M High",   "family": "gpt",      "price": { "input": 5,   "output": 30, "cacheRead": 0.5,  "cacheWrite": 0 } }
  }
}
```

- **`default`** is used when `model` is omitted (`composer-2.5`).
- **`family`** values are free strings; the enforced rule only distinguishes
  `"claude"` from the rest, but storing the real family future-proofs a possible
  "don't review with the same family" rule and documents the catalog.
- **Effort and price**: Cursor prices per *model*, not per effort variant, so all
  `grok-4.5-*` / `gpt-5.5-*` effort ids share one price block. We seed a single
  curated effort id per model; add more entries to widen the menu.
- **No Claude entry** in the seed — delegating to Claude defeats the quota/diversity
  goal. Nothing prevents adding one later.

### 4.2 Extension & override

- A user extends the menu by adding an entry to `models.json` (one line). This
  replaces the old `tierOverrides` mechanism.
- Host-profile override still merges over the base map (same pattern as today's
  `config.ts` merge), keyed on `models` / `default`.
- **No raw-passthrough escape hatch.** A model is callable iff it is in the map.
  This guarantees every callable model is priced and family-tagged.

### 4.3 Schema enum generated at startup

- At server start, load `models.json` and inject the model ids as the `enum` for
  `RUN_INPUT_SCHEMA.model`, and render a recommended-models blurb
  (`id — label — $in/$out`) into the tool description.
- Consequence: `tool-schemas.ts` stops being a static `as const` and becomes a
  builder, e.g. `buildRunInputSchema(config)`, invoked where tools are registered
  (`index.ts`). The LLM caller sees exactly the valid, priced ids.

### 4.4 Resolver (replaces `src/tiers.ts`)

New `resolveModel({ model, requireNonClaude }, config)`:

1. `model = model ?? config.default`.
2. Look up `config.models[model]`; if absent → `ModelNotAllowedError`
   (defense-in-depth behind the enum).
3. If `requireNonClaude` and `family === "claude"` → `NonClaudeViolationError`.
   (Uniform final check — covers both an explicit Claude model and a Claude
   `default`.)
4. Return `{ model, family, price }`.

Deleted: `Tier` type, `TierMap`, `tierOverrides`, `TIERS` allow-list in
`validate.ts`, `DiversityClaudeError`, `TierResolutionError`, the regex.

## 5. Diversity: `requireNonClaude`

- New optional boolean param on `cursor_run` (default `false`).
- Enforced by the resolver (§4.4) with **hard reject** — an explicit
  `NonClaudeViolationError` returned to the caller. Rationale: `capability`'s
  silent downgrade always moves toward the *safer* option; auto-swapping the model
  would silently change cost and quality, so loud is correct for a correctness
  contract.
- The catalog/skill sets `requireNonClaude: true` for reviewer roles (Verifier,
  Design-critic) alongside a recommended non-Claude model.

## 6. Needs-input round-trip

The orchestrator is a suspended LLM during a tool call, so a mid-run question must
round-trip. Make that clean instead of prose-and-manual-relaunch.

- **Signal**: reuse the existing `NEEDS_CONTEXT` status (already in the vocab,
  currently unused). The delegated agent is instructed (prompt convention, same
  mechanism as today's trailing `STATUS:` line parsed in `output.ts`) to end with
  `STATUS: NEEDS_CONTEXT` when it needs an answer to proceed. Its final message
  *is* the question — surfaced in the existing `text` field. No new payload field.
- **Uniform `jobId`**: any run ending in `NEEDS_CONTEXT` is registered/retained as
  a **parked job** (with its `sessionId` + original run context), so the result
  always carries a `jobId` — even a foreground run that never detached for slowness.
  This makes `cursor_answer(jobId, …)` the single resume path for both foreground
  and backgrounded/fanned-out needs-input, with no `sessionId`-only special case.
- **Resume**: new **`cursor_answer(jobId, answer)`** tool.
  - Looks up the parked job in the registry, retrieves its `sessionId` and the
    original run context (isolation, capability, verifyCommands, gate), and resumes
    via `--resume <sessionId>` with `answer` as the prompt.
  - Returns the same shape as `cursor_run` (may be terminal, may be
    `NEEDS_CONTEXT` again, may detach to `RUNNING`/`jobId`).
  - Errors: unknown/expired `jobId` → `NOT_FOUND`; a job not awaiting input →
    rejected ("job is not awaiting an answer").
- **Reliability caveat**: detection leans on the model emitting the `STATUS:` line.
  A weaker model may skip it and just guess; the orchestrator's review of the
  result is the backstop. This is acceptable for the driving use case (every
  delegated plan is reviewed). ACP's typed `ask_question` would remove this
  reliance — recorded as part of the ACP trigger (§9).

## 7. `doctor` tool

New MCP tool, no required input (optional `{ deep?: boolean }` reserved).

Probes and reports:
- **Plugin**: version.
- **Agent**: `cursor-agent` found + `--version`.
- **Account**: parse `agent about` (email, subscription tier, current model) to
  confirm login.
- **Model menu**: compare `models.json` ids against `agent models` (account-scoped
  list). Report ids configured-but-missing (stale) as **warnings**, not failures —
  the model list churns fast (the list already drifted within the same month the
  handoff was written). Prices are not checkable via CLI; note that.

## 8. `delegate` skill + `catalog.md`

- **`plugin/skills/delegate/SKILL.md`** (+ reference): orchestration playbook —
  when to delegate, model picks (`composer-2.5` for bulk; `grok-4.5-xhigh` for
  plan-writing/coding; `gemini-3.5-flash` / `gpt-5.5-high` for diverse review with
  `requireNonClaude: true`), the plan/`NEEDS_CONTEXT`/`cursor_answer` resume flow,
  and the **plan-writer brief template** that conveys the writing-plans methodology
  to the delegate (the driving use case).
- **`config/agents/catalog.md`** rewrite: replace the `tier` column with `model`
  and `requireNonClaude`. The governing principle ("never review an agent's output
  with the same model that produced it") stays, now backed by `requireNonClaude`
  rather than the deleted `diversity` tier.

## 9. Out of scope / deferred

- **ACP** (Agent Client Protocol, `agent acp`). Documented and legitimate
  (`cursor.com/docs/cli/acp`), but its unique value over Path A is exactly one
  case: **foreground + live-human + warm-session mid-run Q&A**. The driving use
  case (orchestrator answers, often fanned-out) does not hit it, and holding N warm
  ACP sessions fights the async/job model. **Trigger to revisit**: the first time a
  live human wants to answer a delegate's question mid-run without ending the tool
  call — or when unreliable `NEEDS_CONTEXT` detection (§6) becomes a real problem.
- **Composer `fast` flag**, **npx distribution**, **MCP elicitation to the human** —
  product decisions, unchanged from the handoff.

## 10. Behavior that stays unchanged

`capability` (ask/plan/write/write-unsandboxed), `isolation`, `session` resume,
`waitMs`/`background`, job tools (`poll`/`cancel`/`wait`/`wait_any`/`wait_all`),
deny-list/safety, `gate`, change-set reporting, pricing computation (now sourced
from `models.json`).

## 11. Testing

- `models.json` load, default resolution, host-profile override merge.
- Schema enum + blurb generated from config.
- Resolver: allowed id; unknown id → `ModelNotAllowedError`; `requireNonClaude`
  rejects a Claude model and a Claude default; passes a non-Claude model; omitted
  model resolves to default.
- `validate.ts`: `model` and `requireNonClaude` field shapes; `tier` no longer
  recognized.
- `NEEDS_CONTEXT` parsed from the `STATUS:` line.
- `cursor_answer`: happy-path resume; unknown `jobId` → `NOT_FOUND`; job not
  awaiting input → rejected.
- `doctor`: id validation against a stubbed `agent models`; account parse from a
  stubbed `agent about`.
- Pricing computation against the new map.

## 12. Migration (hard cut)

Delete `config/tier-map.json`, `src/tiers.ts`, `tests/tiers.test.ts`, the `Tier`
type, `tierOverrides`, and the `tier` schema param. Replace `config/price-map.json`
with `config/models.json`. No `tier` compatibility shim; a caller passing `tier`
gets an unrecognized-param outcome consistent with the current validator's handling
of unknown fields.
