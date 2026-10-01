#!/usr/bin/env bash
# delegate setup: build the CLI, install on PATH, migrate host profile, install the plugin.
#   DRY_RUN=1 ./bin/setup.sh   # preview commands without making changes
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRY_RUN="${DRY_RUN:-0}"

run() {
  if [ "$DRY_RUN" = "1" ]; then
    echo "DRY_RUN: $*"
  else
    "$@"
  fi
}

if ! command -v cargo >/dev/null 2>&1; then
  echo "ERROR: cargo not found on PATH. Install Rust (https://rustup.rs) first." >&2
  exit 1
fi

if ! command -v cursor-agent >/dev/null 2>&1; then
  echo "WARNING: cursor-agent not on PATH. Install it and run 'cursor-agent login' before write jobs." >&2
fi

TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/delegate/target"
run cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" --target-dir "$TARGET_DIR"

INSTALL_BIN="$HOME/.local/bin/delegate"
run mkdir -p "$(dirname "$INSTALL_BIN")"
run cp "$TARGET_DIR/release/delegate" "$INSTALL_BIN"

case ":${PATH}:" in
  *":$HOME/.local/bin:"*) ;;
  *)
    echo "WARNING: $HOME/.local/bin is not on PATH. Add it to your shell profile." >&2
    ;;
esac

CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
OLD_PROFILE="$CONFIG_HOME/cursor-delegate/host-profile.json"
NEW_PROFILE="$CONFIG_HOME/delegate/host-profile.json"
if [ -f "$NEW_PROFILE" ] && [ -f "$OLD_PROFILE" ]; then
  echo "NOTE: host profile exists at both $OLD_PROFILE and $NEW_PROFILE; leaving both untouched."
elif [ ! -f "$NEW_PROFILE" ] && [ -f "$OLD_PROFILE" ]; then
  echo "Migrating host profile to $NEW_PROFILE"
  run mkdir -p "$(dirname "$NEW_PROFILE")"
  run mv "$OLD_PROFILE" "$NEW_PROFILE"
  run rmdir "$CONFIG_HOME/cursor-delegate" 2>/dev/null || true
fi

if command -v claude >/dev/null 2>&1; then
  run claude plugin marketplace add "$REPO_ROOT" --scope user
  run claude plugin install delegate@delegate --scope user
  echo "NOTE: remove a previous cursor-delegate install with: claude plugin uninstall cursor-delegate@cursor-delegate-local && claude plugin marketplace remove cursor-delegate-local"
else
  echo "NOTE: 'claude' CLI not found. Install the plugin manually:"
  echo "  claude plugin marketplace add $REPO_ROOT --scope user"
  echo "  claude plugin install delegate@delegate --scope user"
fi

echo "Done."
