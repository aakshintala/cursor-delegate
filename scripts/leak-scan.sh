#!/usr/bin/env bash
# Scan fixture files for leaked recording-user identity or setup text.
#
#   scripts/leak-scan.sh [PATH...]   (default: tests/fixtures)
#
# Patterns: $USER (record.sh rewrites it to "user", so a hit means the
# redaction missed), plus fixed markers for hook output and
# global-instruction text. The owner's git name and email are public in
# every commit, so they are not patterns.
#
# Prints "<file>: <pattern-name>" per hit — never the matched text, so
# the output itself cannot leak — and exits 1 when any hit is found,
# 0 when clean, 2 on usage errors. There is no allowlist: a fixture
# that quotes a pattern stays uncommitted.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
if [ $# -eq 0 ]; then
  targets=("$here/../tests/fixtures")
else
  targets=("$@")
fi
for t in "${targets[@]}"; do
  [ -e "$t" ] || { echo "leak-scan: no such path: $t" >&2; exit 2; }
done

names=()
patterns=()
add() { [ -n "${2:-}" ] && { names+=("$1"); patterns+=("$2"); } ; }
add "user-login" "${USER:-}"
for m in PONYTAIL QuotaBar 'Global instructions' 'commit freely' mcp__claude_ai Gmail; do
  names+=("marker:$m")
  patterns+=("$m")
done
[ "${#patterns[@]}" -gt 0 ] || exit 0

hit=0
for i in "${!patterns[@]}"; do
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    printf '%s: %s\n' "$f" "${names[$i]}"
    hit=1
  done < <(grep -rFl -- "${patterns[$i]}" "${targets[@]}" 2>/dev/null || true)
done
[ "$hit" -eq 0 ]
