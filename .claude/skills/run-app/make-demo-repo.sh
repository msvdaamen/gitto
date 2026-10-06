#!/usr/bin/env bash
# Makes a small repository to try Gitto on, at $1 (replaced if it's there), with main checked out
# and a branch for each way a merge can go:
#   ff        one commit ahead of main: fast-forwards
#   diverged  branched off earlier, touches another file: a merge commit
#   conflict  changes the line main changed: stops at a conflict in a.txt
#   old       behind main: already up to date
set -euo pipefail
dir="${1:?usage: make-demo-repo.sh <dir>}"
rm -rf "$dir"
mkdir -p "$dir"
cd "$dir"
git init -q -b main
git config user.name "Demo"
git config user.email "demo@example.com"

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
