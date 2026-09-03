#!/usr/bin/env bash
# cursor-delegate setup: build the server and install the plugin with Claude Code at user scope.
# Portable across Linux/macOS. Pure JS build (tsc), no native deps.
#
#   DRY_RUN=1 ./bin/setup.sh   # preview without making changes
set -euo pipefail

NAME="cursor-delegate"
MARKETPLACE="cursor-delegate-local"
PLUGIN="${NAME}@${MARKETPLACE}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRY_RUN="${DRY_RUN:-0}"

run() {
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: $*"
  else
    eval "$@"
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

# 3. Build.
run "cd '$REPO_ROOT' && npm install && npm run build"

# 4. Scaffold a minimal host-profile ONLY if absent (never overwrite).
PROFILE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/cursor-delegate"
PROFILE="$PROFILE_DIR/host-profile.json"
if [ ! -f "$PROFILE" ]; then
  echo "Scaffolding minimal host profile at $PROFILE"
  run "mkdir -p '$PROFILE_DIR'"
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
  "idleMs": 180000
}
JSON
  fi
else
  echo "Host profile already exists at $PROFILE (leaving untouched)."
fi

# 5. Install plugin at user scope (idempotent: marketplace add + plugin install).
if command -v claude >/dev/null 2>&1; then
  run "claude plugin marketplace add '$REPO_ROOT' --scope user"
  run "claude plugin install '$PLUGIN' --scope user"
  echo "Installed '$PLUGIN' with Claude Code (user scope)."
else
  echo "NOTE: 'claude' CLI not found. Install manually from $REPO_ROOT:"
  echo "  claude plugin marketplace add ./ --scope user"
  echo "  claude plugin install $PLUGIN --scope user"
fi

echo "Done."
