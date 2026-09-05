#!/usr/bin/env bash
# cursor-delegate setup: build the server and install the plugin with Claude Code at user scope.
# Portable across Linux/macOS. Pure JS build (tsc), no native deps.
#
#   DRY_RUN=1 ./bin/setup.sh   # preview the commands without making changes
set -euo pipefail

NAME="cursor-delegate"
MARKETPLACE="cursor-delegate-local"
PLUGIN="${NAME}@${MARKETPLACE}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRY_RUN="${DRY_RUN:-0}"

# Execute args directly (no eval): quoting is preserved and arguments are never re-parsed.
run() {
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: $*"
  else
    "$@"
  fi
}

# 1. Resolve THIS machine's node (never bake a path).
NODE_BIN="$(command -v node || true)"
if [ -z "$NODE_BIN" ]; then
  echo "ERROR: node not found on PATH. Install Node.js (>=18) first." >&2
  exit 1
fi
echo "node: $NODE_BIN"

# 2. Warn if cursor-agent is missing (prerequisite: installed + 'cursor-agent login').
if ! command -v cursor-agent >/dev/null 2>&1; then
  echo "WARNING: cursor-agent not on PATH. Install it and run 'cursor-agent login' before using write tools." >&2
fi

# 3. Build. (DRY_RUN prints the shell line instead of executing it.)
BUILD_CMD="cd $(printf %q "$REPO_ROOT") && npm install && npm run build"
if [ "$DRY_RUN" = "1" ]; then
  echo "DRY_RUN: bash -c \"$BUILD_CMD\""
else
  bash -c "$BUILD_CMD"
fi

# 4. Scaffold a minimal host-profile ONLY if absent (never overwrite).
#    Defaults mirror the server's built-in policy (src/index.ts): idleMs 300000,
#    toolIdleMs 1800000.
PROFILE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/cursor-delegate"
PROFILE="$PROFILE_DIR/host-profile.json"
if [ ! -f "$PROFILE" ]; then
  echo "Scaffolding minimal host profile at $PROFILE"
  run mkdir -p "$PROFILE_DIR"
  if [ "$DRY_RUN" != "1" ]; then
    # Optional overrides (all keys optional): set "default" / "models" to extend or
    # override the bundled config/models.json allow-list; the rest are host policy.
    cat > "$PROFILE" <<'JSON'
{
  "requiredDeny": [],
  "promptPreamble": "",
  "verifyCommands": [],
  "gate": "",
  "deadlineMs": 60000,
  "idleMs": 300000,
  "toolIdleMs": 1800000
}
JSON
  fi
else
  echo "Host profile already exists at $PROFILE (leaving untouched)."
fi

# 4b. Scaffold the plugin's MCP-server declaration ONLY if absent (never overwrite).
#     `.mcp.json` is intentionally gitignored: it is host-local, and committing it would make
#     Claude Code ALSO auto-load it as a project-scoped MCP server whenever cwd is inside the
#     repo (double registration). But that means a fresh checkout has none, and `claude plugin
#     install` below would snapshot a plugin with no MCP server (skill loads, tools silently
#     disconnect). Write it here, before the install, so every installed host has it.
#     ${CLAUDE_PLUGIN_ROOT} is resolved by Claude Code at load time to the installed snapshot.
MCP_JSON="$REPO_ROOT/.mcp.json"
if [ ! -f "$MCP_JSON" ]; then
  echo "Scaffolding plugin MCP declaration at $MCP_JSON"
  if [ "$DRY_RUN" != "1" ]; then
    cat > "$MCP_JSON" <<'JSON'
{
  "mcpServers": {
    "cursor-delegate": {
      "type": "stdio",
      "command": "node",
      "args": ["${CLAUDE_PLUGIN_ROOT}/dist/index.js"],
      "timeout": 600000
    }
  }
}
JSON
  fi
else
  echo "Plugin MCP declaration already exists at $MCP_JSON (leaving untouched)."
fi

# 5. Install plugin at user scope (idempotent: marketplace add + plugin install).
if command -v claude >/dev/null 2>&1; then
  run claude plugin marketplace add "$REPO_ROOT" --scope user
  run claude plugin install "$PLUGIN" --scope user
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: skipped install of '$PLUGIN'."
  else
    echo "Installed '$PLUGIN' with Claude Code (user scope)."
  fi
  # 6. Verify the marketplace actually registered.
  if [ "$DRY_RUN" != "1" ]; then
    if claude plugin marketplace list 2>/dev/null | grep -q "$MARKETPLACE"; then
      echo "Verified: marketplace '$MARKETPLACE' is registered."
    else
      echo "WARNING: could not confirm marketplace '$MARKETPLACE' registration. Check 'claude plugin marketplace list'." >&2
    fi
  fi
else
  echo "NOTE: 'claude' CLI not found. Install manually from $REPO_ROOT:"
  echo "  claude plugin marketplace add ./ --scope user"
  echo "  claude plugin install $PLUGIN --scope user"
fi

echo "Done."
