#!/usr/bin/env bash
# Makes a small repository to try Gitto on, at $1, with main checked out and a branch for each way
# a merge can go:
#   ff        one commit ahead of main: fast-forwards
#   diverged  branched off earlier, touches another file: a merge commit
#   conflict  changes the line main changed: stops at a conflict in a.txt
#   old       behind main: already up to date
# A demo repository already at $1 is replaced; anything else there is left alone.
set -euo pipefail
dir="${1:?usage: make-demo-repo.sh <dir>}"
# A demo repository is told by its own config (not a repository's around it), which nothing else has.
if [ -n "$(ls -A "$dir" 2>/dev/null)" ] &&
  [ "$(git --git-dir="$dir/.git" config --local user.email 2>/dev/null)" != demo@example.com ]; then
  echo "$dir is there and isn't a demo repository: pick another path" >&2
  exit 1
fi
rm -rf "$dir"
mkdir -p "$dir"
cd "$dir"
git init -q -b main
git config user.name "Demo"
git config user.email "demo@example.com"
# Not signed even where the environment signs commits: there'd be no key for Demo.
git config commit.gpgsign false
# Environment variables beat the config, and cloud sessions set the author's.
export GIT_AUTHOR_NAME=Demo GIT_AUTHOR_EMAIL=demo@example.com
export GIT_COMMITTER_NAME=Demo GIT_COMMITTER_EMAIL=demo@example.com

printf 'line1\nline2\nline3\n' > a.txt
echo b > b.txt
git add . && git commit -qm "Initial commit"
git branch old && git branch conflict && git branch diverged

sed -i 's/line2/main change/' a.txt && git commit -qam "Change a on main"
git branch ff

git checkout -q ff && echo d > d.txt && git add d.txt && git commit -qm "Add d on ff"
git checkout -q diverged && echo c > c.txt && git add c.txt && git commit -qm "Add c on diverged"
git checkout -q conflict && sed -i 's/line2/conflict change/' a.txt && git commit -qam "Change a on conflict"
git checkout -q main
echo "demo repository at $(pwd)"
