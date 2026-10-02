// How long the RPC server keeps the main thread busy, on a real repository. The server runs here,
// on the main thread, as in Electron's main process; the client runs in a worker thread, as the
// renderer would. Every gap in a 20ms timer is time the main thread couldn't do anything else.
//
//   GITTO_PERF_REPO=~/code/vscode npx tsx packages/rpc/perf/main-thread.ts
// The repository's working tree is changed (and restored); use a throwaway clone.
/* oxlint-disable no-await-in-loop */
/* oxlint-disable unicorn/require-post-message-target-origin -- worker ports, not windows */
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { MessageChannel, Worker } from "node:worker_threads";

import { createDb } from "@gitto/db";

import { createContainer, createRpcHandler } from "../src/server";

const repoPath = resolve(process.env.GITTO_PERF_REPO ?? ".");
const INTERVAL_MS = 20;
/** A frame at 60Hz: blocked for longer, the UI visibly stutters. */
const FRAME_MS = 16;
const STALL_MS = 50;

// --- Measuring --------------------------------------------------------------------------------

interface Phase {
  name: string;
  stalls: number[];
  start: number;
  end?: number;
}
const phases: Phase[] = [];
let current: Phase | undefined;

let last = performance.now();
setInterval(() => {
  const now = performance.now();
  const blocked = now - last - INTERVAL_MS;
  if (blocked >= FRAME_MS) current?.stalls.push(blocked);
  last = now;
}, INTERVAL_MS);

async function phase(name: string, fn: () => Promise<void>) {
  current = { name, stalls: [], start: performance.now() };
  phases.push(current);
  await fn();
  // Let the timer catch up, so a stall at the very end is counted.
  await sleep(INTERVAL_MS * 3);
  current.end = performance.now();
  current = undefined;
}

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Runs a shell command in a child process, so it doesn't count as main-thread time. */
function sh(command: string): Promise<void> {
  return new Promise((done, fail) => {
    const child = spawn("bash", ["-c", command], { cwd: repoPath, stdio: "inherit" });
    child.on("close", (code) => (code === 0 ? done() : fail(new Error(`${command}: ${code}`))));
  });
}

// --- Server and client ------------------------------------------------------------------------

const db = createDb(
  join(mkdtempSync(join(tmpdir(), "gitto-perf-")), "gitto.db"),
  resolve(import.meta.dirname, "../../../apps/electron/migrations"),
);
const context = createContainer(db, { selectFolder: async () => null });
const handler = createRpcHandler();
const { port1, port2 } = new MessageChannel();
handler.upgrade(port1 as never, { context });
port1.start();

const worker = new Worker(new URL("./client.worker.mjs", import.meta.url), {
  workerData: { port: port2 },
  transferList: [port2],
});

let nextId = 0;
const pending = new Map<number, (message: { value?: any; error?: string }) => void>();
const calls: { phase: string; what: string; ms: number; bytes?: number }[] = [];
type WatchEvent = { name: string; event: unknown };
const onEvent: ((event: WatchEvent) => void)[] = [];

worker.on("message", (message) => {
  if (message.type === "result") {
    pending.get(message.id)?.(message);
    pending.delete(message.id);
  } else onEvent.forEach((listener) => listener(message));
});

function request(op: string, ...args: unknown[]): Promise<any> {
  const id = nextId++;
  return new Promise((done, fail) => {
    pending.set(id, (message) =>
      message.error ? fail(new Error(message.error)) : done(message.value),
    );
    worker.postMessage({ id, op, args });
  });
}

async function call(path: string, input?: unknown) {
  const start = performance.now();
  const { bytes, result } = await request("call", path, input);
  calls.push({ phase: current?.name ?? "", what: path, ms: performance.now() - start, bytes });
  return result;
}

// --- What the UI does -------------------------------------------------------------------------

let repositoryId = "";
let statusVersion: string | undefined;

async function status(since = statusVersion) {
  const result = await call("git.status.get", { repositoryId, since });
  if (result.version) statusVersion = result.version;
  return result;
}

/** Opening a repository: refs, log and status at once. */
const openRepository = () =>
  Promise.all([
    call("git.refs.list", { repositoryId }),
    call("git.history.log", { repositoryId }),
    status(undefined),
  ]);

/** What `useRepositoryWatcher` refetches, with the window focused. */
let refetches = 0;
let lastEvent = 0;
onEvent.push(({ name, event }) => {
  lastEvent = performance.now();
  refetches++;
  if (name === "gitDir" && (event as string[]).includes("refs")) void openRepository();
  else void status();
});

/** Waits until the watchers have been quiet for a while, and what they set off has finished. */
async function settle(quietMs = 1500) {
  lastEvent = performance.now();
  while (performance.now() - lastEvent < quietMs || pending.size > 0) await sleep(100);
}

// --- Scenarios --------------------------------------------------------------------------------

console.log(`Measuring ${repoPath}`);
await sh("git status --short | head -5; git rev-list --count HEAD; git ls-files | wc -l");

await phase("open repository (cold)", async () => {
  ({ id: repositoryId } = await call("repository.add", { path: repoPath }));
  await openRepository();
});
await phase("open repository (warm, 3x)", async () => {
  for (let i = 0; i < 3; i++) await openRepository();
});
await phase("start watching git dir + working tree", async () => {
  await request("watch", "gitDir", { repositoryId });
  await request("watch", "workingTree", { repositoryId });
  // The initial walk happens on the watcher's own threads; give it time to finish.
  await sleep(3000);
});
await phase("click through 20 commits", async () => {
  const shas = (
    await new Promise<string>((done) => {
      let out = "";
      const child = spawn("git", ["rev-list", "--max-count=20", "--no-merges", "HEAD"], {
        cwd: repoPath,
      });
      child.stdout.on("data", (chunk) => (out += chunk));
      child.on("close", () => done(out));
    })
  )
    .trim()
    .split("\n");
  for (const sha of shas) {
    await Promise.all([
      call("git.history.commit", { repositoryId, sha }),
      call("git.diff.commitFiles", { repositoryId, sha }),
    ]);
  }
});
await phase("log, largest page (1000)", async () => {
  await call("git.history.log", { repositoryId, limit: 1000 });
});
await phase("edit 5000 files (watcher -> status refetch)", async () => {
  await sh(
    `git ls-files -z -- '*.ts' | head -z -n 5000 | xargs -0 -n 500 sh -c 'for f; do echo "// x" >> "$f"; done' _`,
  );
  await settle();
});
await phase("status, 5000 changed files (full)", async () => {
  await status(undefined);
});
await phase("status, nothing changed since (since=version)", async () => {
  await status();
});
await phase("stage all, 5000 files", async () => {
  await call("git.staging.stageAll", { repositoryId });
  await settle();
});
await phase("unstage all", async () => {
  await call("git.staging.unstageAll", { repositoryId });
  await settle();
});
await phase("revert 5000 files (git checkout -- .)", async () => {
  await sh("git checkout -- .");
  await settle();
});
await phase("npm install: 30000 files in node_modules (ignored)", async () => {
  await sh(
    `mkdir -p node_modules && cd node_modules && for d in $(seq 1 300); do mkdir -p pkg$d/lib; for f in $(seq 1 100); do echo x > pkg$d/lib/f$f.js; done; done`,
  );
  await settle();
});
await phase("build output: 20000 new untracked files", async () => {
  await sh(
    `mkdir -p generated && cd generated && for d in $(seq 1 200); do mkdir -p d$d; for f in $(seq 1 100); do echo x > d$d/f$f.js; done; done`,
  );
  await settle();
});
await phase("status, 20000 untracked files (full)", async () => {
  await status(undefined);
});
await phase("delete build output and node_modules", async () => {
  await sh("rm -rf generated node_modules");
  await settle();
});
await phase("checkout HEAD~300 (refs change -> refetch all)", async () => {
  await sh("git checkout -q --detach HEAD~300");
  await settle();
});
await phase("checkout back", async () => {
  await sh("git checkout -q -");
  await settle();
});

await request("unwatch", "gitDir");
await request("unwatch", "workingTree");

// --- Report -----------------------------------------------------------------------------------

const f = (ms: number) => ms.toFixed(0);
console.log(`\nMain-thread stalls (timer every ${INTERVAL_MS}ms; blocked > ${FRAME_MS}ms counted)`);
console.log(
  ["phase".padEnd(52), "wall", "blocked", "max", `≥${STALL_MS}ms`, `>${FRAME_MS}ms`]
    .map((s, i) => (i ? s.padStart(8) : s))
    .join(""),
);
for (const p of phases) {
  const total = p.stalls.reduce((a, b) => a + b, 0);
  console.log(
    [
      p.name.padEnd(52),
      f(p.end! - p.start),
      f(total),
      f(Math.max(0, ...p.stalls)),
      String(p.stalls.filter((s) => s >= STALL_MS).length),
      String(p.stalls.length),
    ]
      .map((s, i) => (i ? s.padStart(8) : s))
      .join(""),
  );
}

console.log("\nSlowest calls (round trip, ms) and response size");
for (const c of calls.toSorted((a, b) => b.ms - a.ms).slice(0, 15)) {
  console.log(
    `${f(c.ms).padStart(6)}  ${String(c.bytes ?? "").padStart(10)}B  ${c.what.padEnd(22)} ${c.phase}`,
  );
}
console.log(`\nWatcher events that set off a refetch: ${refetches}`);

await worker.terminate();
process.exit(0);
