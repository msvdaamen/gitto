#!/usr/bin/env bash
# Sends one command to the driver running in tmux, starting it there first if it isn't, waits for
# the command to finish and prints what it printed. For driving the app step by step:
#   .claude/skills/run-app/send.sh launch
#   .claude/skills/run-app/send.sh open /tmp/gitto-demo
#   .claude/skills/run-app/send.sh quit
# Exits 1 if the command failed (`ERROR in …`). GITTO_SESSION names the tmux session (gitto).
set -euo pipefail
[ $# -gt 0 ] || { echo "usage: send.sh <command> [argument]" >&2; exit 2; }

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SESSION="${GITTO_SESSION:-gitto}"
LOG="/tmp/gitto-driver-$SESSION.log"

if ! tmux has-session -t "$SESSION" 2>/dev/null; then
  NODE_BIN="$(cat ~/.cache/gitto-run/node-bin 2>/dev/null || true)"
  : >"$LOG"
  tmux new-session -d -s "$SESSION" -x 200 -y 50 \
    "PATH='$NODE_BIN':\"\$PATH\" node '$DIR/driver.mjs' 2>&1 | tee '$LOG'"
  timeout 30 bash -c "until grep -q 'driver ready' '$LOG'; do sleep 0.2; done"
fi

start=$(wc -l <"$LOG")
tmux send-keys -t "$SESSION" -l "$*"
tmux send-keys -t "$SESSION" Enter

# `launch` can take up to 90 s; every other command is done in seconds.
cmd="${1%% *}"
output() { tail -n "+$((start + 1))" "$LOG"; }
deadline=$((SECONDS + 120))
until output | grep -q "^done: $cmd\$"; do
  if ! tmux has-session -t "$SESSION" 2>/dev/null; then
    output
    echo "the driver exited before finishing $cmd" >&2
    exit 1
  fi
  if [ "$SECONDS" -ge "$deadline" ]; then
    output
    echo "timed out waiting for $cmd" >&2
    exit 1
  fi
  sleep 0.2
done
output | grep -v "^done: $cmd\$" || true
! output | grep -q "^ERROR in $cmd:"
