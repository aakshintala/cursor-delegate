#!/usr/bin/env bash
# Parity + footprint gate: replay requests.jsonl against the release binary,
# diff replies (sorted by id, serverInfo dropped) with the TS server's golden
# output, then check idle RSS stays under RSS_MAX_KB.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -q
BIN=target/release/cursor-delegate-mcp
RSS_MAX_KB=${RSS_MAX_KB:-8192}

(cat parity/requests.jsonl; sleep 1) | "$BIN" \
  | jq -s -S 'map(del(.result.serverInfo)) | sort_by(.id)' > parity/actual.json
diff -u parity/expected.json parity/actual.json

# Idle footprint after initialize.
coproc SRV { "$BIN"; }
head -1 parity/requests.jsonl >&"${SRV[1]}"
read -r _ <&"${SRV[0]}"
sleep 1
rss=$(ps -o rss= -p "$SRV_PID" | tr -d ' ')
kill "$SRV_PID"
echo "idle RSS: ${rss} KB (max ${RSS_MAX_KB})"
[ "$rss" -le "$RSS_MAX_KB" ]
