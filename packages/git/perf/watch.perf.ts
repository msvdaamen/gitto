// What the watchers report for everyday actions, on a throwaway clone of each repository, and so
// what the UI refetches: the uncommitted changes (status) for an edited or staged file, everything
// (status, log, refs) for a commit or checkout. See "Refetch after a change on disk" in
// commands.perf.ts for what each costs. More than the action needs is waste.
//
//   GITTO_PERF_REPOS=~/code/vscode pnpm perf watch
// Measurements run one at a time on purpose, so they don't slow each other down.
/* oxlint-disable no-await-in-loop */
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

import { afterAll, describe, it } from "vitest";

import { getStatus } from "../src/features/status/commands";
import { ignoredPaths, watchGitDir, watchWorkingTree } from "../src/features/watch/commands";
import { git, ms, openRepo, perfRepoPaths, printTable } from "./measure";

/** Longer than the watchers' debounce, so an action's reports have all come in. */
const SETTLE_MS = 1000;

const sleep = (duration: number) => new Promise((resolve) => setTimeout(resolve, duration));

/** What the UI refetches for a report (see `useRepositoryWatcher`, with the window focused). */
type Refetch = "uncommitted" | "all";

interface Scenario {
  name: string;
  /** What the UI needs to refetch: nothing if what it shows didn't change. */
  expected: Refetch[];
  /** Runs before the action, and isn't counted. */
  prepare?: (path: string) => void;
  action: (path: string) => void;
}

/** Runs git like another tool would: with optional locks, so `status` refreshes the index. */
function otherTool(cwd: string, ...args: string[]) {
  const env = { ...process.env };
  delete env.GIT_OPTIONAL_LOCKS;
  execFileSync("git", args, { cwd, env, stdio: "ignore" });
}

function trackedFiles(path: string): string[] {
  return git(path, "ls-files", "-z").split("\0").filter(Boolean);
}

const SCENARIOS: Scenario[] = [
  { name: "nothing (idle)", expected: [], action: () => undefined },
  {
    name: "git status by another tool (e.g. VS Code on focus)",
    expected: [],
    action: (path) => otherTool(path, "status"),
  },
  {
    name: "  … 10 times in a row",
    expected: [],
    action: (path) => {
      for (let i = 0; i < 10; i++) otherTool(path, "status");
    },
  },
  {
    name: "git status after a file was touched (content unchanged)",
    expected: [],
    prepare: (path) => {
      const now = new Date();
      utimesSync(join(path, trackedFiles(path)[0]!), now, now);
    },
    action: (path) => otherTool(path, "status"),
  },
  {
    name: "read refs and config (rev-parse, for-each-ref, config)",
    expected: [],
    action: (path) => {
      otherTool(path, "rev-parse", "HEAD");
      otherTool(path, "for-each-ref");
      otherTool(path, "config", "--list");
    },
  },
  {
    name: "save one file",
    expected: ["uncommitted"],
    action: (path) => appendFileSync(join(path, trackedFiles(path)[0]!), "\n// perf\n"),
  },
  {
    name: "save 500 files at once (formatter, search & replace)",
    expected: ["uncommitted"],
    action: (path) => {
      for (const file of trackedFiles(path).slice(0, 500)) {
        appendFileSync(join(path, file), "\n");
      }
    },
  },
  {
    name: "write 2000 files in an ignored folder that existed at start",
    expected: [],
    action: (path) => {
      for (let i = 0; i < 2000; i++) writeFileSync(join(path, "perf-ignored", `${i}.txt`), "x");
    },
  },
  {
    name: "write 2000 files in an ignored folder created after start (build)",
    expected: [],
    action: (path) => {
      mkdirSync(join(path, "perf-build"));
      for (let i = 0; i < 2000; i++) writeFileSync(join(path, "perf-build", `${i}.txt`), "x");
    },
  },
  {
    name: "stage everything",
    expected: ["uncommitted"],
    action: (path) => otherTool(path, "add", "-A"),
  },
  {
    name: "commit",
    expected: ["all"],
    action: (path) => otherTool(path, "commit", "-q", "--no-verify", "-m", "perf"),
  },
  {
    name: "switch to a new branch",
    expected: ["all"],
    action: (path) => otherTool(path, "switch", "-q", "-c", "perf-branch"),
  },
  {
    name: "switch back",
    expected: ["all"],
    action: (path) => otherTool(path, "switch", "-q", "-"),
  },
];

const clones: string[] = [];
afterAll(() => {
  for (const clone of clones) rmSync(clone, { recursive: true, force: true });
});

describe.each(perfRepoPaths())("%s", (source) => {
  it("watcher", async () => {
    const root = mkdtempSync(join(tmpdir(), "gitto-watch-perf-"));
    clones.push(root);
    const path = join(root, basename(source));
    execFileSync("git", ["clone", "-q", "--local", source, path]);
    git(path, "config", "user.name", "Perf");
    git(path, "config", "user.email", "perf@example.com");
    git(path, "config", "commit.gpgsign", "false");
    appendFileSync(join(path, ".git/info/exclude"), "\nperf-ignored/\nperf-build/\n");
    mkdirSync(join(path, "perf-ignored"));

    const repo = await openRepo(path);
    const start = performance.now();
    const ignored = await ignoredPaths(repo);
    const ignoredMs = performance.now() - start;

    const reports: { at: number; refetch: Refetch }[] = [];
    // Every report has the UI ask for the status again, which the git directory's watcher goes by.
    const refetched = (refetch: Refetch) => {
      reports.push({ at: performance.now(), refetch });
      void getStatus(repo).catch(() => undefined);
    };
    await getStatus(repo);
    const controller = new AbortController();
    const watching = Promise.all([
      (async () => {
        for await (const changes of watchGitDir(repo, controller.signal)) {
          refetched(changes.includes("refs") ? "all" : "uncommitted");
        }
      })(),
      (async () => {
        for await (const _ of watchWorkingTree(repo, controller.signal)) refetched("uncommitted");
      })(),
    ]);
    // Let the watchers start, and anything the clone left behind settle.
    await sleep(SETTLE_MS * 2);
    reports.length = 0;

    const rows = [];
    for (const scenario of SCENARIOS) {
      scenario.prepare?.(path);
      await sleep(SETTLE_MS);
      const before = reports.length;
      const actionStart = performance.now();
      scenario.action(path);
      const actionEnd = performance.now();
      await sleep(SETTLE_MS);
      const seen = reports.slice(before);
      const refetches = seen.map((report) => report.refetch);
      const wasteful =
        refetches.length > scenario.expected.length ||
        (refetches.includes("all") && !scenario.expected.includes("all"));
      rows.push({
        action: scenario.name,
        refetches: summarize(refetches),
        expected: summarize(scenario.expected),
        "": wasteful ? "✗ extra refetch" : "",
        "first report after": seen[0] ? `${ms(seen[0].at - actionEnd)}ms` : "",
        "action took": `${ms(actionEnd - actionStart)}ms`,
      });
    }
    controller.abort();
    await watching;

    printTable(
      `Watchers on a clone of ${basename(source)} (listing ${ignored.length} ignored paths took ${ms(ignoredMs)}ms)`,
      rows,
    );
  });
});

/** E.g. `2× uncommitted, all`; `-` for nothing. */
function summarize(refetches: Refetch[]): string {
  if (refetches.length === 0) return "-";
  const counts = new Map<Refetch, number>();
  for (const refetch of refetches) counts.set(refetch, (counts.get(refetch) ?? 0) + 1);
  return [...counts]
    .map(([refetch, count]) => (count > 1 ? `${count}× ${refetch}` : refetch))
    .join(", ");
}
