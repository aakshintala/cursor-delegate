# Idea: one delegate server for every agent CLI

Status: parked (2026-09-25). Do after the native rewrite lands.

## What

Generalize cursor-delegate into a delegate server that drives cursor-agent,
Codex, Pi, and Fiber (once `fiber ask` ships) behind one set of tools.

One server instead of four also saves memory: each stdio MCP server is a
process per client session.

## Shape

- Routing: each entry in `config/models.json` gains
  `"backend": "cursor" | "codex" | "pi" | "fiber"`. Callers still pass a model;
  the backend follows from it. No new tool parameter.
- Per backend: an argv builder, a stream-line parser that yields the existing
  progress snapshot and a normalized result, a resume strategy for
  needs-input, and a doctor check.
- Tools rename `cursor_*` → `delegate_*` with a major version bump. No aliases;
  there is one consumer.

## Where it will leak

- Resume / needs-input. cursor-agent resumes by chat id; each other CLI has
  its own session mechanism. Design the interface against two real backends
  (Cursor + Codex), not one.
- Sandboxing. Not every CLI has one. Where it doesn't, worktree isolation is
  the only guard, and the safety policy needs a per-backend capability flag.
- Current Cursor coupling: the raw result type in the backend interface, the
  argv builder in the runner, and Cursor wording throughout the tool schemas
  and doctor.

## Fiber

Good fit. `fiber ask --prompt-file brief.md` writes the session's events as
JSONL on stdout and ends with a `fiber_exited` verdict line; a missing verdict
means the process died (`~/work/fiber/docs/invocation.md`). Wait for `ask` to
exist.

## Unverified

Codex and Pi headless flags (`codex exec --json`, Pi's JSON mode). Check
these first.

## Order

1. Cursor + Codex, to force the right interface.
2. Pi.
3. Fiber.
