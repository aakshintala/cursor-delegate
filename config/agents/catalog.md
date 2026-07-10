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
