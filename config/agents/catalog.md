# Agent catalog (convention layer)

These "agents" are **convention tuples over `cursor_run`** — the host agent (the controller)
constructs each call. Nothing here is Cursor-side configuration; this is documentation, not code.

**Governing principle:** *never review an agent's output with the same model that produced it.*
The `diversity` tier is contractually non-Claude, so it gives an uncorrelated second opinion.

| Agent | tier | capability | isolation | prompt shape |
|---|---|---|---|---|
| **Verifier** | `diversity` | `ask` | None | adversarial: *try to refute the claim / find the bug* |
| **Triager** | `cheap-bulk` | `ask` | None | classify / route / summarize an issue |
| **Design-critic** | `diversity` | `plan` | None | critique a design, surface risks, propose alternatives |
| **Codemod** | `coding-specialist` | `write` | CallerProvided | mechanical, well-scoped edit across a tree |
| **Re-implementer** | `coding-specialist` | `write` | CallerProvided | rebuild a component from a spec |
| **SP-implementer** | `cheap-bulk` | `write` | CallerProvided | implement a single planned task |
| **SP spec-reviewer** | `standard` | `ask` | None | review a spec for gaps (different engine than implementer) |
| **SP quality-reviewer** | `standard` | `ask` | None | review an implementation for quality (different engine) |

Notes:
- Implementers use `CallerProvided` isolation so writes land in a known working tree and participate
  in the same-path write lock.
- Always pass `verifyCommands` to bound what an implementer may run, and a `gate` to enforce the
  postcondition the tool itself checks.
- Reviewers run on `standard`/`diversity` — deliberately a *different* engine from whatever produced
  the artifact under review.
