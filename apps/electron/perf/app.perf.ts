// The app end to end, as close to how it runs as it gets without Electron: the renderer, built for
// production, in Chromium, and the main process's router in this process, exchanging the same RPC
// messages (main → renderer as Server-Sent Events, which arrive as plain message events, like a
// MessagePort's). On a throwaway clone of each repository, it times what users do (save a file,
// commit in Gitto or in another tool, fetch, scroll) and reports, per action, what the renderer
// asked for, how much came back, when the result showed, and how busy each side was.
//
//   GITTO_PERF_REPOS=~/code/vscode pnpm perf app
//
// Needs Chromium: `pnpm --filter @gitto/electron exec playwright install chromium`, or set
// GITTO_PERF_CHROMIUM to one. GITTO_PERF_SCENARIOS (comma-separated) runs only those scenarios, and
// GITTO_PERF_PROFILE=<scenario> writes a CPU profile of the renderer during its first run.
// Measurements run one at a time on purpose, so they don't slow each other down.
/* oxlint-disable no-await-in-loop */
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { appendFileSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type ServerResponse } from "node:http";
import { createRequire } from "node:module";
import type { AddressInfo } from "node:net";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
// The global `performance` is typed as the DOM's here, which the page's code needs.
import { performance as nodePerformance } from "node:perf_hooks";

import { createDb } from "@gitto/db";
import { createContainer, createRpcHandler } from "@gitto/rpc/server";
import { chromium, type CDPSession, type Page } from "playwright";
import { describe, it } from "vitest";

const ROOT = resolve(import.meta.dirname, "../../..");
const UI = join(ROOT, "apps/ui");
const MIGRATIONS = join(ROOT, "apps/electron/migrations");
/** How often each scenario runs. */
const RUNS = Number(process.env.GITTO_PERF_RUNS ?? 5);
const SCENARIOS = process.env.GITTO_PERF_SCENARIOS?.split(",").filter(Boolean) ?? [];
const PROFILE = process.env.GITTO_PERF_PROFILE;
/** Settled once no message has crossed for this long: longer than the watchers' debounce. */
const QUIET_MS = 1200;

/** The repositories in GITTO_PERF_REPOS (comma-separated), or this one. */
function perfRepoPaths(): string[] {
  const paths = process.env.GITTO_PERF_REPOS?.split(",").filter(Boolean) ?? [];
  if (paths.length === 0) return [ROOT];
  return paths.map((path) => resolve(path.replace(/^~(?=\/|$)/, homedir())));
}

const sleep = (duration: number) => new Promise((done) => setTimeout(done, duration));

function git(cwd: string, ...args: string[]): string {
  return execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 1 << 30 }).trim();
}

/** What the page has, besides the DOM, for the harness. */
interface PerfWindow {
  /** Sends a message to the main process. */
  toMain(data: string): void;
  /** Durations of the long animation frames since the last scenario started. */
  frames: number[];
  /** When the history first showed 21 rows, since the page started loading. */
  rowsShown?: number;
  /** When the history first showed (or stopped showing) what a scenario waits for, since it started. */
  shown?: Promise<number>;
}

/**
 * A clone of `source` to change freely, with what makes a repository slow to show: its remote
 * branches (a plain clone only has the source's local ones), a few stashes, and uncommitted
 * changes, some of them staged.
 */
function cloneRepo(source: string): { path: string; files: string[] } {
  const path = join(mkdtempSync(join(tmpdir(), "gitto-app-perf-")), basename(source));
  git(dirname(path), "clone", "-q", "--local", source, path);
  git(
    path,
    "fetch",
    "-q",
    "--no-tags",
    source,
    "+refs/remotes/*:refs/remotes/*",
    "^refs/remotes/*/HEAD",
  );
  git(path, "config", "user.name", "Perf");
  git(path, "config", "user.email", "perf@example.com");
  git(path, "config", "commit.gpgsign", "false");

  const files = git(path, "ls-files", "-z")
    .split("\0")
    .filter((file) => /\.(ts|tsx|js|md|txt|c|h|py|go|rs|java)$/.test(file))
    .slice(0, 12);
  if (files.length < 12) throw new Error(`${source} has too few text files to change.`);
  for (const [i, file] of files.slice(0, 3).entries()) {
    appendFileSync(join(path, file), `\n// perf stash ${i}\n`);
    git(path, "stash", "push", "-q", "-m", `perf stash ${i}`);
  }
  for (const file of files.slice(3, 11)) appendFileSync(join(path, file), "\n// perf\n");
  git(path, "add", "--", ...files.slice(3, 6));
  return { path, files };
}

/** A port nothing listens on. */
async function freePort(): Promise<number> {
  const server = createServer().listen(0);
  await once(server, "listening");
  const { port } = server.address() as AddressInfo;
  server.close();
  return port;
}

/** Runs Vite (the UI's) with `args`, in the UI's folder. */
function vite(args: string[], options: { wait?: boolean } = {}) {
  const bin = join(
    dirname(createRequire(join(UI, "package.json")).resolve("vite/package.json")),
    "bin/vite.js",
  );
  // NODE_ENV as `pnpm package` has it: Vitest sets it to `test`, which would build Solid's dev mode.
  const env = { ...process.env, NODE_ENV: "production" };
  if (options.wait) {
    execFileSync(process.execPath, [bin, ...args], { cwd: UI, env, stdio: "ignore" });
    return undefined;
  }
  return spawn(process.execPath, [bin, ...args], { cwd: UI, env, stdio: "ignore" });
}

type RpcPort = Parameters<ReturnType<typeof createRpcHandler>["upgrade"]>[0];

/**
 * Stands in for Electron's MessagePortMain: the router listens on it, and what it posts goes to
 * the page.
 */
class BridgePort {
  private readonly listeners: ((event: { data: unknown }) => void)[] = [];
  constructor(private readonly send: (data: string) => void) {}
  on(type: string, listener: (event: { data: unknown }) => void) {
    if (type === "message") this.listeners.push(listener);
  }
  postMessage(data: unknown) {
    if (typeof data !== "string") throw new Error("Only text messages cross the bridge.");
    this.send(data);
  }
  deliver(data: string) {
    for (const listener of this.listeners) listener({ data });
  }
  start() {}
}

/** The messages crossing the bridge: which procedures were called, and what came back. */
class Traffic {
  calls = new Map<string, number>();
  bytes = 0;
  last = performance.now();

  reset() {
    this.calls = new Map();
    this.bytes = 0;
  }

  /** Waits until nothing has crossed for QUIET_MS. */
  async settle() {
    while (performance.now() - this.last < QUIET_MS) await sleep(100);
  }
}

interface Result {
  shown: number;
  settled: number;
  calls: string;
  rpcs: number;
  kb: number;
  task: number;
  script: number;
  layout: number;
  longestFrame: number;
  mainBusy: number;
}

async function rendererTime(cdp: CDPSession) {
  const { metrics } = await cdp.send("Performance.getMetrics");
  const get = (name: string) => metrics.find((metric) => metric.name === name)?.value ?? 0;
  return {
    task: get("TaskDuration") * 1000,
    script: get("ScriptDuration") * 1000,
    layout: (get("LayoutDuration") + get("RecalcStyleDuration")) * 1000,
  };
}

/** Starts watching for the history to show (or stop showing) `text`; see `shownAfter`. */
async function watchHistory(page: Page, text: string, present: boolean) {
  await page.evaluate(
    ([awaited, shows]) => {
      const start = performance.now();
      (window as unknown as PerfWindow).shown = new Promise<number>((done) => {
        const check = () => {
          const list = document.querySelector('[role="listbox"][aria-label="Commit history"]');
          if (!!list?.textContent?.includes(awaited) !== shows) return false;
          done(performance.now() - start);
          return true;
        };
        if (check()) return;
        const observer = new MutationObserver(() => check() && observer.disconnect());
        observer.observe(document.body, { subtree: true, childList: true, characterData: true });
      });
    },
    [text, present] as const,
  );
}

/** How long after `watchHistory` the history showed what it waited for. */
function shownAfter(page: Page): Promise<number> {
  return page.evaluate(() => (window as unknown as PerfWindow).shown!);
}

describe.each(perfRepoPaths())("%s", (source) => {
  it("app", { timeout: 60 * 60 * 1000 }, async () => {
    const { path, files } = cloneRepo(source);
    const port = await freePort();
    vite(["build", ...(PROFILE ? ["--minify", "false"] : [])], { wait: true });
    const preview = vite(["preview", "--port", String(port), "--strictPort"])!;
    const browser = await chromium.launch({ executablePath: process.env.GITTO_PERF_CHROMIUM });
    const db = createDb(join(dirname(path), "gitto.db"), MIGRATIONS);
    // Main → renderer: messages wait here until the page connects.
    const pending: string[] = [];
    let events: ServerResponse | undefined;
    const sse = createServer((_request, response) => {
      response.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "Access-Control-Allow-Origin": "*",
      });
      events = response;
      for (const data of pending.splice(0)) response.write(`data: ${data}\n\n`);
    }).listen(0);
    try {
      await once(sse, "listening");
      const ssePort = (sse.address() as AddressInfo).port;
      const context = createContainer(db, { selectFolder: async () => null });
      const repository = await context.repositoryService.addRepository(path);

      // The app's CSP only lets the renderer talk to itself; the bridge is another port.
      const page = await browser.newPage({
        viewport: { width: 1400, height: 900 },
        bypassCSP: true,
      });
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Performance.enable");
      // The app watches the working tree while its window has focus, as it does on a desktop.
      await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });

      const traffic = new Traffic();
      const bridge = new BridgePort((data) => {
        // Messages are JSON, so they have no line breaks of their own to escape.
        traffic.bytes += data.length;
        traffic.last = performance.now();
        if (events) events.write(`data: ${data}\n\n`);
        else pending.push(data);
      });
      await page.exposeBinding("perfToMain", (_source, data: string) => {
        traffic.last = performance.now();
        const procedure = /"u":"\/([^"?]+)/.exec(data)?.[1]?.replaceAll("/", ".");
        if (procedure) traffic.calls.set(procedure, (traffic.calls.get(procedure) ?? 0) + 1);
        bridge.deliver(data);
      });
      await page.addInitScript((fromMainPort: number) => {
        const perf = window as unknown as PerfWindow & { perfToMain(data: string): void };
        // Where the preload would take the renderer's port and hand it to the main process.
        window.addEventListener("message", (event) => {
          if (event.data !== "gitto:rpc-connect") return;
          const [rpcPort] = event.ports;
          rpcPort!.addEventListener("message", (message) => perf.perfToMain(message.data));
          rpcPort!.start();
          new EventSource(`http://localhost:${fromMainPort}/`).addEventListener(
            "message",
            (message) =>
              // oxlint-disable-next-line unicorn/require-post-message-target-origin -- a MessagePort's.
              rpcPort!.postMessage(message.data),
          );
        });
        perf.frames = [];
        new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) perf.frames.push(entry.duration);
        }).observe({ type: "long-animation-frame" });
        new MutationObserver((_, observer) => {
          if (document.querySelectorAll('[role="option"]').length <= 20) return;
          perf.rowsShown = performance.now();
          observer.disconnect();
        }).observe(document, { subtree: true, childList: true });
      }, ssePort);
      createRpcHandler().upgrade(bridge as unknown as RpcPort, { context });

      const results = new Map<string, Result[]>();
      /** Times `action` (and `shown`, when what it waits for showed), if it's one to run. */
      async function measure(
        name: string,
        action: () => Promise<unknown>,
        shown?: () => Promise<number>,
      ) {
        if (SCENARIOS.length && !SCENARIOS.includes(name) && name !== "open repository") return;
        await traffic.settle();
        traffic.reset();
        await page.evaluate(() => ((window as unknown as PerfWindow).frames = []));
        const before = await rendererTime(cdp);
        const utilization = nodePerformance.eventLoopUtilization();
        const profiling = PROFILE === name && !results.has(name);
        if (profiling) {
          await cdp.send("Profiler.enable");
          await cdp.send("Profiler.setSamplingInterval", { interval: 100 });
          await cdp.send("Profiler.start");
        }
        const start = performance.now();
        traffic.last = start;
        await action();
        const shownMs = shown ? await shown() : Number.NaN;
        await traffic.settle();
        const settled = traffic.last - start;
        const mainBusy = nodePerformance.eventLoopUtilization(utilization).active;
        if (profiling) {
          const { profile } = await cdp.send("Profiler.stop");
          const file = join(ROOT, `${name.replaceAll(/\W+/g, "-")}.cpuprofile`);
          writeFileSync(file, JSON.stringify(profile));
          console.log(`Wrote a CPU profile of the renderer to ${file}`);
        }
        const after = await rendererTime(cdp);
        const frames = await page.evaluate(() => (window as unknown as PerfWindow).frames);
        const runs = results.get(name) ?? [];
        results.set(name, runs);
        runs.push({
          shown: shownMs,
          settled,
          calls: [...traffic.calls].map(([call, n]) => (n > 1 ? `${call}×${n}` : call)).join(" "),
          rpcs: [...traffic.calls.values()].reduce((sum, n) => sum + n, 0),
          kb: traffic.bytes / 1024,
          task: after.task - before.task,
          script: after.script - before.script,
          layout: after.layout - before.layout,
          longestFrame: Math.max(0, ...frames),
          mainBusy,
        });
      }
      /** Runs an action that isn't timed, and waits for what it sets off. */
      async function untimed(action: () => void) {
        traffic.last = performance.now();
        action();
        await traffic.settle();
      }

      await measure(
        "open repository",
        () => page.goto(`http://localhost:${port}/${repository.id}`),
        async () => {
          await page.waitForFunction(
            () => (window as unknown as PerfWindow).rowsShown !== undefined,
            null,
            {
              timeout: 120_000,
            },
          );
          // Since the page started loading, as the page measures it.
          return page.evaluate(() => (window as unknown as PerfWindow).rowsShown!);
        },
      );
      // Let the first commit-graph write and the working tree watch settle.
      await traffic.settle();
      await sleep(3000);

      for (let run = 0; run < RUNS; run++) {
        await measure("save a file", async () => {
          appendFileSync(join(path, files[11]!), `// run ${run}\n`);
        });

        const message = `perf commit ${run}`;
        await measure(
          "commit in another tool",
          async () => {
            await watchHistory(page, message, true);
            const sha = git(path, "commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", message);
            git(path, "update-ref", "-m", `commit: ${message}`, "HEAD", sha);
          },
          () => shownAfter(page),
        );
        await measure(
          "reset in another tool",
          async () => {
            await watchHistory(page, message, false);
            git(path, "update-ref", "-m", "reset", "HEAD", "HEAD~1");
          },
          () => shownAfter(page),
        );

        await measure("fetch, nothing new", async () => git(path, "fetch", "-q", "."));

        const ownMessage = `perf commit in Gitto ${run}`;
        await measure(
          "commit in Gitto",
          async () => {
            await page.getByPlaceholder("Summary of your changes").fill(ownMessage);
            const button = page.getByRole("button", { name: /^Commit \d+ files?/ });
            await button.waitFor();
            await watchHistory(page, ownMessage, true);
            await button.click();
          },
          () => shownAfter(page),
        );
        // Undone, so the next run commits the same files; only if it did commit.
        if (git(path, "log", "-1", "--format=%s") === ownMessage) {
          await untimed(() => git(path, "reset", "-q", "--soft", "HEAD~1"));
        }

        await measure("scroll the history", async () => {
          await page.locator("main").first().hover();
          for (const delta of [400, -400]) {
            for (let i = 0; i < 30; i++) {
              await page.mouse.wheel(0, delta);
              await sleep(16);
            }
          }
        });
      }

      printTable(`${describeRepo(path)} (median of ${RUNS} runs)`, [...results].map(summarize));
    } finally {
      await browser.close();
      preview.kill();
      sse.close();
      db.$client.close();
      rmSync(dirname(path), { recursive: true, force: true });
    }
  });
});

/** A scenario's row in the table: medians, and the calls of its last run. */
function summarize([scenario, runs]: [string, Result[]]): Record<string, string> {
  const med = (key: keyof Result) => median(runs.map((run) => run[key] as number));
  return {
    scenario,
    "shown ms": ms(med("shown")),
    "settled ms": ms(med("settled")),
    rpcs: String(med("rpcs")),
    "KB to ui": ms(med("kb")),
    "ui busy": ms(med("task")),
    "ui script": ms(med("script")),
    "ui layout": ms(med("layout")),
    "longest frame": ms(med("longestFrame")),
    "main busy": ms(med("mainBusy")),
    calls: runs.at(-1)!.calls,
  };
}

/** Rounded to the millisecond (or kilobyte); empty if not measured. */
function ms(value: number): string {
  return Number.isNaN(value) ? "" : value.toFixed(0);
}

function median(values: number[]): number {
  const sorted = values.toSorted((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? Number.NaN;
}

/** A one-line summary of the repository's size, which is what the timings depend on. */
function describeRepo(path: string): string {
  const commits = git(path, "rev-list", "--count", "--all");
  const files = git(path, "ls-files", "-z").split("\0").length - 1;
  const refs = git(path, "for-each-ref", "--format=x").split("\n").length;
  return `${basename(path)}: ${commits} commits, ${files} files, ${refs} refs`;
}

/** Prints rows as an aligned table: the first column left-aligned, the rest right-aligned. */
function printTable(title: string, rows: Record<string, string>[]): void {
  const columns = Object.keys(rows[0] ?? {});
  const widths = columns.map((column) =>
    Math.max(column.length, ...rows.map((row) => row[column]?.length ?? 0)),
  );
  const line = (cells: string[]) =>
    cells
      .map((cell, i) =>
        i === 0 || i === columns.length - 1 ? cell.padEnd(widths[i]!) : cell.padStart(widths[i]!),
      )
      .join("  ");
  console.log(
    `\n${title}\n${line(columns)}\n${rows.map((row) => line(columns.map((c) => row[c] ?? ""))).join("\n")}`,
  );
}
