#!/usr/bin/env bash
# Gets a fresh Linux container (e.g. a Claude Code cloud session) ready to run Gitto headless:
# Node 24, the dependencies, Electron's binary, Xvfb, Playwright and a production build.
# Safe to run again: each step is skipped when it's done already. Run it from anywhere.
#
# Afterwards, put Node 24 first on PATH in each shell that runs the driver or builds:
#   export PATH="$(cat ~/.cache/gitto-run/node-bin):$PATH"
set -euo pipefail

ROOT="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
CACHE="$HOME/.cache/gitto-run"
mkdir -p "$CACHE"

step() { printf '\n== %s\n' "$*"; }

step "Node 24 (the repo needs >=24; on Node 22 the build leaves node:sqlite out and the app hangs)"
major() { "$1" -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0; }
NODE_BIN=""
if [ "$(major node)" -ge 24 ]; then
  NODE_BIN="$(dirname "$(command -v node)")"
else
  export NVM_DIR="${NVM_DIR:-/opt/nvm}"
  if [ ! -s "$NVM_DIR/nvm.sh" ]; then
    export NVM_DIR="$HOME/.nvm"
    [ -s "$NVM_DIR/nvm.sh" ] || curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
  fi
  # nvm's scripts trip over `set -u`.
  set +u
  # shellcheck disable=SC1091
  . "$NVM_DIR/nvm.sh"
  nvm install 24 >/dev/null
  NODE_BIN="$(dirname "$(nvm which 24)")"
  set -u
fi
echo "$NODE_BIN" > "$CACHE/node-bin"
export PATH="$NODE_BIN:$PATH"
echo "node $(node -v) at $NODE_BIN"

step "Dependencies"
(cd "$ROOT" && pnpm install --frozen-lockfile)

step "Electron's binary (pnpm may skip its download)"
ELECTRON_DIR="$(cd "$ROOT/apps/electron/node_modules/electron" && pwd -P)"
if ! (cd "$ROOT/apps/electron" && node -e 'require("electron")' >/dev/null 2>&1); then
  (cd "$ELECTRON_DIR" && node install.js)
fi
(cd "$ROOT/apps/electron" && node -p 'require("electron")')

step "Xvfb and the libraries Electron loads"
if ! command -v Xvfb >/dev/null; then
  apt-get update -qq
  apt-get install -y -qq xvfb libnss3 libgbm1 libgtk-3-0 libxss1 libxkbcommon0 \
    libatk-bridge2.0-0 libcups2 libdrm2 >/dev/null
  apt-get install -y -qq libasound2t64 >/dev/null 2>&1 || apt-get install -y -qq libasound2 >/dev/null
fi
command -v Xvfb

step "Playwright (drives the app; globally installed on cloud sessions, else cached here)"
if ! NODE_PATH="$(npm root -g)" node -e 'require("playwright-core")' >/dev/null 2>&1 \
  && ! NODE_PATH="$(npm root -g)" node -e 'require("playwright")' >/dev/null 2>&1; then
  npm install --silent --prefix "$CACHE" playwright-core
fi
echo ok

step "Production build into apps/electron/.vite (rerun after changing the code)"
(cd "$ROOT/apps/electron" && pnpm package >/dev/null)
grep -q 'from "node:sqlite"' "$ROOT/apps/electron/.vite/build/main.js" \
  || { echo "node:sqlite wasn't kept external: was the build run on Node <24?" >&2; exit 1; }
echo "built apps/electron/.vite"

printf '\nReady. Next:\n  export PATH="$(cat ~/.cache/gitto-run/node-bin):$PATH"\n  node %s/.claude/skills/run-app/driver.mjs\n' "$ROOT"
