#!/usr/bin/env bash
# Scan fixture files for leaked recording-user identity or setup text.
#
#   scripts/leak-scan.sh [PATH...]   (default: tests/fixtures)
#
# Patterns: the user's home directory (/Users/$USER, /home/$USER; a bare
# $USER is too common a word, e.g. "runner" in CI), plus fixed markers for hook output and
# global-instruction text. The owner's git name and email are public in
# every commit, so they are not patterns.
#
# Prints "<file>: <pattern-name>" per hit — never the matched text, so
# the output itself cannot leak — and exits 1 when any hit is found,
# 0 when clean, 2 on usage or read errors. There is no allowlist: a fixture
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
[ -n "${USER:-}" ] && add "home-dir" "/Users/$USER" && add "home-dir" "/home/$USER"
for m in PONYTAIL QuotaBar 'Global instructions' 'commit freely' mcp__claude_ai Gmail; do
  names+=("marker:$m")
  patterns+=("$m")
done
[ "${#patterns[@]}" -gt 0 ] || exit 0

# grep exits 0 on a match, 1 on none, 2 on an error. An error fails the
# scan: an unread file is not a clean file.
hit=0
for i in "${!patterns[@]}"; do
  files=$(grep -rFl -- "${patterns[$i]}" "${targets[@]}")
  case $? in
    0) printf '%s\n' "$files" | sed "s|\$|: ${names[$i]}|"; hit=1 ;;
    1) ;;
    *) echo "leak-scan: grep failed for pattern ${names[$i]}" >&2; exit 2 ;;
  esac
done
[ "$hit" -eq 0 ]
