# cursor-delegate

An MCP stdio server that lets a host agent (Claude Code, or any MCP client) delegate
coding/research tasks to **Cursor's models** by shelling out to the local `cursor-agent` CLI in
headless (`--print`) mode.

See [`spec.md`](./spec.md) for the full design. Highlights:

- **Multi-model tiering** — `cheap-bulk` / `standard` / `coding-specialist` / `diversity`
  (diversity is contractually non-Claude).
- **Capability modes** — `ask` / `plan` (read-only) vs `write` / `write-unsandboxed`.
- **Fail-closed deny-list** — write calls are refused unless the host's cursor-agent deny-list
  contains every required pattern.
- **Async job model** — fast tasks return synchronously; slow tasks detach and hand back a `jobId`
  you `cursor_poll` / `cursor_wait` on. Live progress streams while a call blocks.
- **Ground-truth verification** — the tool computes the git change-set itself, runs an optional
  postcondition `gate`, and surfaces stderr on failure.
- **Same-path write serialization** — concurrent writes to one tree are refused (`BUSY`), not
  interleaved.

## Tools

`cursor_run`, `cursor_poll`, `cursor_cancel`, `cursor_wait`, `cursor_wait_any`, `cursor_wait_all`.

## Prerequisites (per machine)

1. `cursor-agent` installed and logged in (`cursor-agent status`).
2. A host profile at `~/.config/cursor-delegate/host-profile.json` (scaffolded by setup).
3. For any **write** capability: the host deny-list merged into `~/.cursor/cli-config.json`
   `permissions.deny`. These files are per-machine — do not copy them between hosts.

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
| `config/tier-map.json` (bundled) | default `Tier -> {backend, model}` |
| `config/price-map.json` (bundled) | default per-million-token prices |
| `~/.config/cursor-delegate/host-profile.json` | overrides + policy (override path via `$CURSOR_DELEGATE_HOST_PROFILE`) |
| `~/.cursor/cli-config.json` | Cursor's own `permissions.deny` — checked before writes |

See [`config/agents/catalog.md`](./config/agents/catalog.md) for the reusable "agent" convention
tuples over `cursor_run`.
