# cursor-delegate

An MCP stdio server that lets a host agent (Claude Code, or any MCP client) delegate
coding/research tasks to **Cursor's models** by shelling out to the local `cursor-agent` CLI in
headless (`--print`) mode.

See [`spec.md`](./spec.md) for the original reproduction spec and
[`docs/superpowers/specs/2026-07-09-cursor-delegate-model-layer-design.md`](./docs/superpowers/specs/2026-07-09-cursor-delegate-model-layer-design.md)
for the current model-layer design. Highlights:

- **Curated model allow-list** — a single `config/models.json` maps each callable id to a label,
  a `family` tag, and `$/MTok` prices. A model is callable iff it is in the map; the `model` enum
  and a recommended-models blurb are generated into the `cursor_run` schema at startup. Default is
  `composer-2.5` when `model` is omitted.
- **Uncorrelated review contract** — `requireNonClaude: true` **hard-rejects** if the resolved
  model's family is `claude` (no silent swap), so a reviewer never shares the producer's family.
- **Capability modes** — `ask` / `plan` (read-only: no edits, but they run read-only shell such as
  `git show` for branch-only content) vs `write` / `write-unsandboxed`. Every mode runs `--force`
  so a headless agent never blocks on an approval prompt.
- **Fail-closed deny-list** — **every** call (read-only included) is refused unless the host's
  cursor-agent deny-list contains every required pattern. Because `--force` lets even `ask`/`plan`
  run non-denied shell commands, the deny-list is their only guard.
- **Async job model** — fast tasks return synchronously; slow tasks detach and hand back a `jobId`
  you `cursor_poll` / `cursor_wait` on. Live progress streams while a call blocks.
- **Needs-input round-trip** — a delegate that ends with `STATUS: NEEDS_CONTEXT` is retained as a
  parked job (carrying its `sessionId` + run context); `cursor_answer(jobId, answer)` resumes it via
  `--resume`. The result's `text` **is** the delegate's question.
- **Ground-truth verification** — the tool computes the git change-set itself, runs an optional
  postcondition `gate`, and surfaces stderr on failure.
- **Same-path write serialization** — concurrent writes to one tree are refused (`BUSY`), not
  interleaved.
- **Setup diagnostics** — a `doctor` tool probes the plugin version, `cursor-agent` binary +
  login, and configured-vs-account model-menu drift (missing ids are warnings, not failures).

## Tools

`cursor_run`, `cursor_poll`, `cursor_cancel`, `cursor_wait`, `cursor_wait_any`, `cursor_wait_all`,
`cursor_answer`, `doctor`.

For orchestration guidance (when to delegate, model picks, the plan-writer brief, the needs-input
resume flow), see the [`delegate` skill](./plugin/skills/delegate/SKILL.md).

## Prerequisites (per machine)

1. `cursor-agent` installed and logged in (`cursor-agent status`).
2. A host profile at `~/.config/cursor-delegate/host-profile.json` (scaffolded by setup).
3. For **any** capability (read-only `ask`/`plan` included, since they run `--force` shell): the
   host deny-list merged into `~/.cursor/cli-config.json` `permissions.deny`. These files are
   per-machine — do not copy them between hosts.

## Build & install

```bash
./bin/setup.sh          # build + scaffold profile + register at user scope
DRY_RUN=1 ./bin/setup.sh # preview without changes
```

Or manually:

```bash
npm install && npm run build
claude mcp add cursor-delegate -s user -- "$(command -v node)" "$PWD/dist/index.js"
```

As a Claude Code plugin, point your plugin config at `plugin/plugin.json` (which references
`plugin/.mcp.json`).

## Develop

```bash
npm test         # offline unit tests (fakes for spawn/clock/git/config)
npm run test:live # opt-in: real cursor-agent (must be installed + logged in)
npm run build    # tsc -> dist/
```

## Config

| file | role |
|---|---|
| `config/models.json` (bundled) | model allow-list: `default` + per-id `{label, family, price}` (`$/MTok`) |
| `~/.config/cursor-delegate/host-profile.json` | overrides + policy; may merge `default` / `models` and set `requiredDeny`, `gate`, deadlines (override path via `$CURSOR_DELEGATE_HOST_PROFILE`) |
| `~/.cursor/cli-config.json` | Cursor's own `permissions.deny` — checked before every run (all caps carry `--force`) |

See [`config/agents/catalog.md`](./config/agents/catalog.md) for the reusable "agent" convention
tuples over `cursor_run`.
