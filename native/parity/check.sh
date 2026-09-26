#!/usr/bin/env bash
# Parity + footprint gate: replay requests.jsonl against the release binary,
# diff replies (sorted by id, serverInfo dropped) with the TS server's golden
# output, then check idle RSS stays under RSS_MAX_KB.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q
BIN=target/release/cursor-delegate-mcp
RSS_MAX_KB=${RSS_MAX_KB:-4096}

(cat parity/requests.jsonl; sleep 1) | "$BIN" \
  | jq -s -S 'map(del(.result.serverInfo)) | sort_by(.id)' > parity/actual.json
diff -u parity/expected.json parity/actual.json

# Idle footprint after initialize. Keep stdin open with a FIFO (macOS bash has no coproc).
fifo=$(mktemp -u) && mkfifo "$fifo"
"$BIN" < "$fifo" > /dev/null &
pid=$!
exec 3> "$fifo"
head -1 parity/requests.jsonl >&3
sleep 1
rss=$(ps -o rss= -p "$pid" | tr -d ' ')
exec 3>&-
wait "$pid" 2>/dev/null || true
rm -f "$fifo"
echo "idle RSS: ${rss} KB (max ${RSS_MAX_KB})"
[ "$rss" -le "$RSS_MAX_KB" ]
