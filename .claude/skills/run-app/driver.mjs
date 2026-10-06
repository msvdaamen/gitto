// Drives Gitto's Electron app headless, for agents: launches the production build (see setup.sh)
// on its own Xvfb display and runs commands read from stdin, one per line. Pipe a script in to run
// it and exit, or run it in tmux and send commands one at a time. `help` lists the commands.
import { execFileSync, spawn } from "node:child_process";
import * as fs from "node:fs";
import { createRequire } from "node:module";
import * as os from "node:os";
import * as path from "node:path";
import * as readline from "node:readline";
import * as tty from "node:tty";

const ROOT = path.resolve(import.meta.dirname, "../../..");
const APP_DIR = path.join(ROOT, "apps/electron");
const CACHE = path.join(os.homedir(), ".cache/gitto-run");
const SHOT_DIR = process.env.SCREENSHOT_DIR || "/tmp/gitto-shots";
fs.mkdirSync(SHOT_DIR, { recursive: true });

/** Playwright, from setup.sh's cache or the global packages. */
function loadPlaywright() {
  const roots = [path.join(CACHE, "node_modules")];
  try {
    roots.push(execFileSync("npm", ["root", "-g"], { encoding: "utf8" }).trim());
  } catch {}
  for (const root of roots) {
    for (const name of ["playwright-core", "playwright"]) {
      try {
        return createRequire(path.join(root, "noop.js"))(name);
      } catch {}
    }
  }
  throw new Error("Playwright isn't installed: run .claude/skills/run-app/setup.sh");
}
const { _electron: electron } = loadPlaywright();

let xvfb = null;
let app = null;
let page = null;
let profile = null;

/** Starts Xvfb on a free display, unless DISPLAY is set; xvfb-run's auth trips Electron up. */
async function ensureDisplay() {
  if (process.env.DISPLAY) return process.env.DISPLAY;
  let n = 90;
  while (fs.existsSync(`/tmp/.X${n}-lock`)) n++;
  xvfb = spawn("Xvfb", [`:${n}`, "-screen", "0", "1400x900x24", "-ac"], { stdio: "ignore" });
  for (let i = 0; i < 50 && !fs.existsSync(`/tmp/.X11-unix/X${n}`); i++) {
    // oxlint-disable-next-line no-await-in-loop -- polling until Xvfb's socket is there.
    await new Promise((r) => setTimeout(r, 100));
  }
  return `:${n}`;
}

const need = () => {
  if (!page) throw new Error("launch first");
  return page;
};
const visible = (locator) => locator.filter({ visible: true }).first();
/**
 * Chromium's dbus complaints on every start and click (there's no session bus in a container), and
 * the inspector Playwright attaches through.
 */
const NOISE =
  /dbus|Failed to connect to the bus|object_proxy|Debugger (listening|attached|ending)|getting-started\/debugging|^\s*$/;

const COMMANDS = {
  /** Launches the app with a fresh profile (no repositories), or `launch keep` to keep the last. */
  async launch(arg) {
    if (app) return console.log("already launched");
    const electronBin = createRequire(path.join(APP_DIR, "package.json"))("electron");
    if (!fs.existsSync(path.join(APP_DIR, ".vite/build/main.js"))) {
      throw new Error("no build: run .claude/skills/run-app/setup.sh");
    }
    profile = path.join(os.tmpdir(), "gitto-run-profile");
    if (arg !== "keep") fs.rmSync(profile, { recursive: true, force: true });
    const display = await ensureDisplay();
    app = await electron.launch({
      executablePath: electronBin,
      args: ["--no-sandbox", `--user-data-dir=${profile}`, APP_DIR],
      env: { ...process.env, DISPLAY: display },
      timeout: 90_000,
    });
    for (const stream of [app.process().stdout, app.process().stderr]) {
      readline.createInterface({ input: stream }).on("line", (line) => {
        if (!NOISE.test(line)) console.log(`[main] ${line}`);
      });
    }
    page = await app.firstWindow();
    page.on("console", (m) => {
      if (m.type() === "error" || m.type() === "warning")
        console.log(`[ui:${m.type()}] ${m.text()}`);
    });
    page.on("pageerror", (e) => console.log(`[ui:exception] ${e.message}`));
    await page.getByText("Open repository").first().waitFor({ timeout: 30_000 });
    console.log("launched", page.url());
  },

  /** Opens the repository at `dir` (absolute path), answering the folder picker with it. */
  async open(dir) {
    need();
    const target = path.resolve(dir);
    await app.evaluate(({ dialog }, d) => {
      dialog.showOpenDialog = async () => ({ canceled: false, filePaths: [d] });
    }, target);
    // On the home page, or the tabs' + once a repository is open.
    const button = page.getByRole("button", { name: "Open repository" }).filter({ visible: true });
    if (await button.count()) await button.first().click();
    else await visible(page.getByTitle("Open repository")).click();
    await page.locator("[data-branch]").first().waitFor({ timeout: 15_000 });
    console.log("opened", target);
  },

  /** Screenshot of the window, to SCREENSHOT_DIR/<name>.png. Look at it. */
  async ss(name) {
    const file = path.join(SHOT_DIR, `${name || Date.now()}.png`);
    await need().screenshot({ path: file });
    console.log("screenshot:", file);
  },

  /** Clicks the visible element with this text, exactly, else containing it. */
  async "click-text"(text) {
    const exact = need().getByText(text, { exact: true }).filter({ visible: true });
    await ((await exact.count()) ? exact.first() : visible(page.getByText(text))).click({
      timeout: 5000,
    });
    console.log("clicked", JSON.stringify(text));
  },

  /** `click-role <role> <name>`, e.g. `click-role button Commit merge`, `click-role menuitem Merge ff into main`. */
  async "click-role"(arg) {
    const [role, ...rest] = arg.split(" ");
    const name = rest.join(" ");
    await visible(need().getByRole(role, { name })).click({ timeout: 5000 });
    console.log("clicked", role, JSON.stringify(name));
  },

  /** Clicks the button with this title (icon-only buttons), e.g. `click-title Open repository`. */
  async "click-title"(title) {
    await visible(need().getByTitle(title)).click({ timeout: 5000 });
    console.log("clicked title", JSON.stringify(title));
  },

  /** Right-clicks a branch's row in the sidebar, by its full ref name: `rclick refs/heads/main`. */
  async rclick(ref) {
    await visible(need().locator(`[data-branch="${ref}"]`)).click({
      button: "right",
      timeout: 5000,
    });
    console.log("right-clicked", ref);
  },

  /** Right-clicks a branch's label in the history instead, by its full ref name. */
  async "rclick-label"(ref) {
    // The sidebar's row comes first; the label is the second element marked with it.
    const label = need().locator(`[data-branch="${ref}"]`).filter({ visible: true }).nth(1);
    await label.dispatchEvent("contextmenu", { button: 2 });
    console.log("right-clicked label", ref);
  },

  /** Lists the open menu's items, marking disabled ones. */
  async menu() {
    const items = await need()
      .getByRole("menuitem")
      .evaluateAll((els) =>
        els.map(
          (e) =>
            e.textContent.trim() +
            (e.getAttribute("aria-disabled") === "true" || e.hasAttribute("data-disabled")
              ? " [disabled]"
              : ""),
        ),
      );
    console.log("menu:", JSON.stringify(items));
  },

  async press(key) {
    await need().keyboard.press(key);
    console.log("pressed", key);
  },
  async type(text) {
    await need().keyboard.type(text, { delay: 20 });
    console.log("typed", JSON.stringify(text));
  },
  async sleep(ms) {
    await new Promise((r) => setTimeout(r, Number(ms) || 1000));
    console.log("slept", ms || 1000);
  },
  /** Waits up to 10s for visible text. */
  async wait(text) {
    await visible(need().getByText(text)).waitFor({ timeout: 10_000 });
    console.log("found", JSON.stringify(text));
  },
  /** innerText of the page, or of the first element matching a CSS selector. */
  async text(sel) {
    console.log(
      await need().evaluate(
        (s) => (s ? document.querySelector(s) : document.body)?.innerText ?? "(none)",
        sel || null,
      ),
    );
  },
  /** Evaluates JavaScript in the page and prints the result as JSON. */
  async eval(expr) {
    console.log(JSON.stringify(await need().evaluate(expr)));
  },

  async quit() {
    if (app) await app.close().catch(() => {});
    app = null;
    page = null;
    if (xvfb && xvfb.exitCode === null) {
      const exited = new Promise((r) => xvfb.once("exit", r));
      xvfb.kill();
      await exited;
    }
    xvfb = null;
  },

  help() {
    console.log("commands:", Object.keys(COMMANDS).join(", "));
  },
};

// Electron must not get the terminal's stdin; read it through its own stream. A terminal's (in
// tmux) through a tty stream: a file stream's read blocks a thread, which process.exit waits for.
const stdinFd = fs.openSync("/dev/stdin", "r");
const input = tty.isatty(stdinFd)
  ? new tty.ReadStream(stdinFd)
  : fs.createReadStream(null, { fd: stdinFd });
const rl = readline.createInterface({
  input,
  output: process.stdout,
  prompt: "driver> ",
  terminal: false,
});
/** Runs one line of input: a command and its argument. */
async function run(line) {
  const trimmed = line.trim();
  if (!trimmed || trimmed.startsWith("#")) return;
  const space = trimmed.indexOf(" ");
  const cmd = space < 0 ? trimmed : trimmed.slice(0, space);
  const arg = space < 0 ? "" : trimmed.slice(space + 1);
  const fn = COMMANDS[cmd];
  if (!fn) console.log(`unknown command: ${cmd} (try help)`);
  else {
    try {
      await fn(arg);
    } catch (e) {
      console.log(`ERROR in ${cmd}: ${e.message.split("\n")[0]}`);
    }
  }
  console.log(`done: ${cmd}`);
  if (cmd === "quit") process.exit(0);
}

// One at a time, in order, however fast the lines come.
let queue = Promise.resolve();
rl.on("line", (line) => {
  queue = queue.then(() => run(line));
});
rl.on("close", async () => {
  await queue;
  await COMMANDS.quit();
  process.exit(0);
});
process.on("SIGINT", async () => {
  await COMMANDS.quit();
  process.exit(130);
});
console.log("gitto driver ready (help lists the commands)");
