#!/usr/bin/env bash
# Sends one command to the driver running in tmux, starting it there first if it isn't, waits for
# the command to finish and prints what it printed. For driving the app step by step:
#   .claude/skills/run-app/send.sh launch
#   .claude/skills/run-app/send.sh open /tmp/gitto-demo
#   .claude/skills/run-app/send.sh quit
# Exits 1 if the command failed (`ERROR in …`). GITTO_SESSION names the tmux session (gitto), so
# drivers can run side by side, each with its own app.
set -euo pipefail
[ $# -gt 0 ] || { echo "usage: send.sh <command> [argument]" >&2; exit 2; }

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SESSION="${GITTO_SESSION:-gitto}"
LOG="/tmp/gitto-driver-$SESSION.log"
# How many commands were sent to this driver: the driver runs them in order, and each prints one
# `done:` line, so this command's is that many `done:` lines in, whatever came before it.
SENT="$LOG.sent"

if ! tmux has-session -t "$SESSION" 2>/dev/null; then
  NODE_BIN="$(cat ~/.cache/gitto-run/node-bin 2>/dev/null || true)"
  : >"$LOG"
  echo 0 >"$SENT"
  tmux new-session -d -s "$SESSION" -x 200 -y 50 \
    "GITTO_SESSION='$SESSION' PATH='$NODE_BIN':\"\$PATH\" node '$DIR/driver.mjs' 2>&1 | tee '$LOG'"
  timeout 30 bash -c "until grep -q 'driver ready' '$LOG'; do sleep 0.2; done"
fi

dones() { grep -c '^done: ' "$LOG" || true; }
# Not there for a session started some other way: then what its driver has done so far.
n=$(($(cat "$SENT" 2>/dev/null || dones) + 1))
echo "$n" >"$SENT"
tmux send-keys -t "$SESSION" -l "$*"
tmux send-keys -t "$SESSION" Enter

# What the command printed: the lines after the done line before its own, up to its own.
output() { awk -v n="$n" '/^done: /{ d++; if (d == n) exit; next } d == n - 1' "$LOG"; }

# `launch` can take a couple of minutes on a slow machine; every other command, seconds. Commands
# from an earlier call that timed out finish first.
deadline=$((SECONDS + 300))
until [ "$(dones)" -ge "$n" ]; do
  # Read again once the session's gone: quit prints its done line and exits straight after.
  if ! tmux has-session -t "$SESSION" 2>/dev/null && [ "$(dones)" -lt "$n" ]; then
    output
    echo "the driver exited before finishing $1" >&2
    exit 1
  fi
  if [ "$SECONDS" -ge "$deadline" ]; then
    output
    echo "timed out waiting for $1; it's still running, and the next command waits for it" >&2
    exit 1
  fi
  sleep 0.2
done
out="$(output)"
[ -z "$out" ] || printf '%s\n' "$out"
case "$out" in
  "ERROR in "* | *$'\n'"ERROR in "*) exit 1 ;;
esac
