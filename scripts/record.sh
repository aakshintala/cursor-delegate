#!/usr/bin/env bash
# Run one prompt through a backend's raw headless CLI and save stdout/stderr as a parser fixture.
#
#   scripts/record.sh BACKEND MODEL CAPABILITY NAME [CWD] [SESSION] < prompt
#
# BACKEND: cursor | pi | claude. CAPABILITY: read-only | read-write (pi rejects read-only).
# SESSION resumes that backend session; pi and claude get a fresh id when it is omitted.
# CANCEL_AFTER=N sends the CLI SIGTERM after N seconds.
# Writes tests/fixtures/BACKEND/NAME.{stdout,stderr,argv}; prints the session id (if chosen) and exit code.
set -uo pipefail

[ $# -ge 4 ] || { sed -n 2,9p "$0"; exit 2; }
backend=$1 model=$2 cap=$3 name=$4 cwd=${5:-$PWD} session=${6:-}
here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../tests/fixtures/$backend"
mkdir -p "$out"

prompt="$(cat)

End your final message with a single trailing line that is exactly one of: STATUS: DONE, STATUS: DONE_WITH_CONCERNS, STATUS: BLOCKED, STATUS: NEEDS_CONTEXT, or STATUS: ERROR. When you need an answer from the orchestrator before you can proceed, put your question in the message body and end with STATUS: NEEDS_CONTEXT."

case "$cap" in read-only|read-write) ;; *) echo "bad capability: $cap" >&2; exit 2 ;; esac

case "$backend" in
  cursor)
    argv=(cursor-agent --print --output-format stream-json --trust --approve-mcps --force --model "$model" --workspace "$cwd")
    [ "$cap" = read-only ] && argv+=(--mode ask) || argv+=(--sandbox disabled)
    [ -n "$session" ] && argv+=(--resume "$session") ;;
  pi)
    [ "$cap" = read-only ] && { echo "pi cannot enforce read-only" >&2; exit 2; }
    session=${session:-$(uuidgen | tr A-Z a-z)}
    argv=(pi -p --mode json --model "$model" --session-id "$session") ;;
  claude)
    [ "$cap" = read-only ] && mode=plan || mode=auto
    argv=(claude -p --output-format stream-json --verbose --model "$model" --permission-mode "$mode")
    if [ -n "$session" ]; then argv+=(--resume "$session")
    else session=$(uuidgen | tr A-Z a-z); argv+=(--session-id "$session"); fi ;;
  *) echo "bad backend: $backend" >&2; exit 2 ;;
esac

printf '%q ' "${argv[@]}" > "$out/$name.argv"; echo >> "$out/$name.argv"
cd "$cwd" || exit 2
[ "$backend" = cursor ] && argv+=(-- "$prompt") && prompt=
"${argv[@]}" < <(printf '%s' "$prompt") > "$out/$name.stdout" 2> "$out/$name.stderr" &
pid=$!
[ -n "${CANCEL_AFTER:-}" ] && sleep "$CANCEL_AFTER" && kill -TERM "$pid"
wait "$pid"
code=$?
# Redact the user's setup (see redact.jq) and home-directory name; non-JSON lines pass through.
for f in "$out/$name".{stdout,stderr,argv}; do
  jq -rR -L "$here" 'include "redact"; (fromjson? | redact | tojson) // .' "$f" | sed "s/$USER/user/g" > "$f.tmp" && mv "$f.tmp" "$f"
done
echo "session=$session exit=$code fixture=$out/$name.stdout"
exit $code
