---
name: run-app
description: Build, launch and drive the Gitto Electron app headless on Linux (e.g. a Claude Code cloud session) — to try a change in the real app, take screenshots, click through the UI, or check a feature end to end. Use when asked to run, start, test in the app, or screenshot Gitto.
---

# Running Gitto headless

Gitto is an Electron app. In a container there's no screen, so it's run from its production build
on an Xvfb display and driven with Playwright through `driver.mjs`, which reads commands from stdin.
All paths are relative to the repository root.

## 1. Set up (once per container, ~1 min; ~15 s when it's mostly there)

```bash
.claude/skills/run-app/setup.sh
export PATH="$(cat ~/.cache/gitto-run/node-bin):$PATH"   # Node 24, in every new shell
```

It installs Node 24 (with nvm) when needed, the dependencies, Electron's binary, Xvfb, Playwright,
and builds into `apps/electron/.vite`. Each step is skipped when it's done already.

**After changing code, rebuild** (the driver runs the build, not the sources; ~5 s):

```bash
node .claude/skills/run-app/build.mjs
```

It builds the `.vite` targets in `forge.config.ts` as `pnpm package` does, without also packaging
the app into `out/`, which takes half a minute more and isn't used.

## 2. A repository to open

The app starts with no repositories (a fresh profile each `launch`). Use any repository by absolute
path, or make the demo one, whose branches cover each way a merge goes (`ff`, `diverged`,
`conflict`, `old`; see the script):

```bash
.claude/skills/run-app/make-demo-repo.sh /tmp/gitto-demo
```

Running it again resets the demo repository; it won't replace a folder that's something else. Its
commits are by "Demo" (it sets `GIT_AUTHOR_*` and `GIT_COMMITTER_*`, which cloud sessions set
too), but the commits you make in the app are by whoever the environment says.

## 3. Drive it

**A script**: pipe commands in; the app closes when they're done (a whole run takes seconds).

```bash
node .claude/skills/run-app/driver.mjs <<'EOF'
launch
open /tmp/gitto-demo
rclick refs/heads/ff
menu
click-role menuitem Merge ff into main
sleep 1500
ss after-merge
EOF
```

**Step by step**, keeping the app open between commands: `send.sh` runs the driver in tmux (it
starts it the first time), sends one command, waits for it to finish and prints its output. It
exits 1 if the command failed. To run two apps side by side, give each its own `GITTO_SESSION`
(the tmux session, and the app's profile).

```bash
.claude/skills/run-app/send.sh launch
.claude/skills/run-app/send.sh open /tmp/gitto-demo
.claude/skills/run-app/send.sh ss opened
.claude/skills/run-app/send.sh quit
```

A merge that stops at a conflict, resolved in the conflict view and committed (the banner above
the history shows the merge under way, the view opens on the first conflicted file):

```bash
node .claude/skills/run-app/driver.mjs <<'EOF'
launch
open /tmp/gitto-demo
rclick refs/heads/conflict
click-role menuitem Merge conflict into main
wait Keep both
ss conflict
click-role button Keep both
click-role button Mark resolved
sleep 1000
ss resolved
click-role button Commit merge
sleep 1500
ss merged
EOF
git -C /tmp/gitto-demo log --oneline --graph --all
```

In the conflict view, each conflict has **Keep ours**, **Keep theirs** and **Keep both**, and the
file **Keep all of ours** and **Keep all of theirs**; **Mark resolved** stages it. The banner has
**Resolve conflicts**, **Abort** and **Commit merge** (**Continue** for a rebase or cherry-pick).

Every command prints `done: <command>` when finished, and `ERROR in <command>: …` if it failed.
Screenshots go to `/tmp/gitto-shots/<name>.png` (`SCREENSHOT_DIR` to change). **Read them**: a
command succeeding doesn't mean the UI shows what it should. Check git's side with `git -C <repo>`.

| Command                       | What it does                                                                                     |
| ----------------------------- | ------------------------------------------------------------------------------------------------ |
| `launch`                      | Starts the app with a fresh profile (`launch keep` keeps the last one's repositories)            |
| `open <dir>`                  | Opens the repository at the absolute path `dir` (answers the folder picker), also as another tab |
| `ss [name]`                   | Screenshot of the window                                                                         |
| `click-text <text>`           | Clicks the visible element with that text (exact, else containing)                               |
| `click-role <role> <name>`    | E.g. `click-role button Commit merge`, `click-role menuitem Merge ff into main`                  |
| `click-title <title>`         | Clicks an icon-only button by its tooltip                                                        |
| `rclick <ref>`                | Right-clicks a branch in the sidebar by full ref name: `rclick refs/heads/main`                  |
| `rclick-label <ref>`          | Right-clicks the branch's label in the history instead                                           |
| `menu`                        | Lists the open menu's items, with `[disabled]` on disabled ones                                  |
| `press <key>` / `type <text>` | Keyboard, e.g. `press Escape`                                                                    |
| `wait <text>`                 | Waits up to 10 s for visible text                                                                |
| `sleep [ms]`                  | Waits (default 1000)                                                                             |
| `text [css]`                  | Prints the page's text, or an element's, e.g. `text [role=alertdialog]`                          |
| `eval <js>`                   | Runs JavaScript in the page, prints the result as JSON                                           |
| `quit`                        | Closes the app (and the driver's Xvfb)                                                           |

The main process's logs (`[main] …`, e.g. RPC errors) and the UI's errors and warnings (`[ui:…]`)
are printed as they come.

## Gotchas

- **Node 24 or newer for the build.** On Node 22 the main process's bundle gets an empty stub for
  `node:sqlite` (Node 22 doesn't list it as a built-in), so opening the database throws on startup,
  and the error dialog blocks the main process: it looks like a hang, and Playwright times out
  connecting. `setup.sh` checks for this.
- **The production build, not `pnpm start`.** `electron-forge start` launches its own Electron that
  Playwright can't attach to, and packaged builds (`out/`) turn the inspector off with fuses. The
  driver runs the `.vite` build with the plain Electron binary.
- **No `xvfb-run` needed.** The driver starts its own `Xvfb :90+ -ac` (no X authority to pass
  along) unless `DISPLAY` is set, and stops it on `quit`.
- **Things load after the click.** Diffs, the conflict view and the history fill in a moment later:
  `sleep 1000` (or `wait <text>`) before `ss`, or the screenshot shows an empty pane.
- **No native folder picker.** `open` replaces `dialog.showOpenDialog` in the main process to
  answer with the path.
- **Leftover runs.** Don't kill processes with `pkill -f <pattern>`: it matches the shell running
  the command too. Use `quit`, or `kill` by PID. A driver stuck in `send.sh`'s tmux session goes with
  `tmux kill-session -t gitto`; the next `send.sh` starts a new one.
- Chromium's dbus errors are expected (there's no session bus) and filtered out.
