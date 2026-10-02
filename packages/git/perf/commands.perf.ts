// Times every read the UI makes on real repositories. Each is split into the git process, which
// runs alongside the backend process serving the UI, and the work that keeps that one busy (so its
// other calls wait): starting git, parsing its output, checking it against the contract and
// encoding it for the renderer. `decode` is the renderer's share: turning the message back into
// objects.
//
//   GITTO_PERF_REPOS=~/code/vscode,~/code/linux pnpm perf
// Measurements run one at a time on purpose, so they don't slow each other down.
/* oxlint-disable no-await-in-loop */
import { monitorEventLoopDelay } from "node:perf_hooks";

import { StandardRPCJsonSerializer, StandardRPCSerializer } from "@orpc/client/standard";
import type { AnyContractProcedure } from "@orpc/contract";
import { describe, it } from "vitest";

import type { Repo } from "../src/core/repo";
import { getCommitFiles, getLineCounts } from "../src/features/diff/commands";
import { DiffContract } from "../src/features/diff/contract";
import type { FileChange } from "../src/features/diff/schema";
import { getCommit, getLog } from "../src/features/history/commands";
import { HistoryContract } from "../src/features/history/contract";
import { listRefs } from "../src/features/refs/commands";
import { RefsContract } from "../src/features/refs/contract";
import { getStatus } from "../src/features/status/commands";
import { StatusContract } from "../src/features/status/contract";
import { ignoredPaths } from "../src/features/watch/commands";
import {
  bytes,
  describeRepo,
  git,
  median,
  ms,
  openRepo,
  percentile,
  perfRepoPaths,
  printTable,
  RUNS,
} from "./measure";

const serializer = new StandardRPCSerializer(new StandardRPCJsonSerializer());

interface Operation {
  name: string;
  run: (repo: Repo) => Promise<unknown>;
  /** The procedure that returns it, to validate and encode the result the way oRPC does. */
  procedure?: AnyContractProcedure;
  /**
   * What change on disk refetches it (see `useRepositoryWatcher`): `uncommitted` for an edited or
   * staged file, `refs` for a commit or checkout (which refetches the uncommitted ones too).
   */
  refetchedOn?: "uncommitted" | "refs";
}

interface Timing {
  total: number;
  git: number;
  spawn: number;
  parse: number;
  validate: number;
  encode: number;
  decode: number;
  bytes: number;
}

/** How many changed files the details panel shows at once, and so counts the lines of. */
const FILES_IN_VIEW = 30;

/**
 * @param head the commit HEAD is at
 * @param inView the changed files in view in the details panel, whose lines it counts
 */
function operations(head: string, inView: FileChange[]): Operation[] {
  return [
    {
      name: "status",
      run: (repo) => getStatus(repo),
      procedure: StatusContract.get,
      refetchedOn: "uncommitted",
    },
    {
      name: `line counts (${inView.length} files in view)`,
      run: async (repo) => (inView.length > 0 ? getLineCounts(repo, "unstaged", inView) : []),
      procedure: DiffContract.lineCounts,
      refetchedOn: "uncommitted",
    },
    {
      name: "log 200",
      run: (repo) => getLog(repo, { limit: 200, skip: 0 }),
      procedure: HistoryContract.log,
      refetchedOn: "refs",
    },
    {
      name: "log 200, no commit-graph",
      run: (repo) => withoutCommitGraph(() => getLog(repo, { limit: 200, skip: 0 })),
      procedure: HistoryContract.log,
    },
    {
      name: "log 1000",
      run: (repo) => getLog(repo, { limit: 1000, skip: 0 }),
      procedure: HistoryContract.log,
    },
    {
      name: "log 200, skip 10000",
      run: (repo) => getLog(repo, { limit: 200, skip: 10_000 }),
      procedure: HistoryContract.log,
    },
    {
      name: "refs",
      run: (repo) => listRefs(repo),
      procedure: RefsContract.list,
      refetchedOn: "refs",
    },
    {
      name: "commit HEAD",
      run: (repo) => getCommit(repo, head),
      procedure: HistoryContract.commit,
    },
    {
      name: "commit files HEAD",
      run: (repo) => getCommitFiles(repo, head),
      procedure: DiffContract.commitFiles,
    },
    { name: "ignored paths (watcher)", run: (repo) => ignoredPaths(repo) },
  ];
}

/** Runs git as if the repository had no commit-graph file, as freshly cloned ones don't. */
async function withoutCommitGraph<T>(run: () => Promise<T>): Promise<T> {
  const config = {
    GIT_CONFIG_COUNT: "1",
    GIT_CONFIG_KEY_0: "core.commitGraph",
    GIT_CONFIG_VALUE_0: "false",
  };
  Object.assign(process.env, config);
  try {
    return await run();
  } finally {
    for (const key of Object.keys(config)) delete process.env[key];
  }
}

/** What returning `result` from `procedure` costs: oRPC's output validation, then the message. */
async function respond(procedure: AnyContractProcedure | undefined, result: unknown) {
  const start = performance.now();
  const schema = procedure?.["~orpc"].outputSchema;
  const validated = schema ? await schema["~standard"].validate(result) : { value: result };
  if ("issues" in validated && validated.issues)
    throw new Error("Output doesn't match the contract");
  const validate = performance.now();
  const message = JSON.stringify(serializer.serialize(validated.value));
  const encode = performance.now();
  serializer.deserialize(JSON.parse(message));
  const decode = performance.now();
  return {
    validate: validate - start,
    encode: encode - validate,
    decode: decode - encode,
    bytes: message.length,
  };
}

async function measure(repo: Repo, operation: Operation): Promise<Timing> {
  let spawn = 0;
  let lastOutput = 0;
  // Times the synchronous part of each spawn, and when git's last output came in: everything
  // after that, until the command resolves, is parsing.
  const timed: Repo = {
    ...repo,
    read: (args, options) => {
      const start = performance.now();
      const output = repo.read(args, options);
      spawn += performance.now() - start;
      return output.then((out) => {
        lastOutput = performance.now();
        return out;
      });
    },
  };

  const start = performance.now();
  const result = await operation.run(timed);
  const end = performance.now();
  return {
    total: end - start,
    git: lastOutput - start,
    spawn,
    parse: end - lastOutput,
    ...(await respond(operation.procedure, result)),
  };
}

/**
 * Everything a change on disk refetches, at once, like the UI does. `busy` is how much of that
 * time the process serving the UI was occupied (so other calls waited); `stall` its longest block.
 */
async function measureRefresh(repo: Repo, refetched: Operation[]) {
  const delay = monitorEventLoopDelay({ resolution: 1 });
  delay.enable();
  const utilization = performance.eventLoopUtilization();
  const start = performance.now();
  await Promise.all(
    refetched.map(async (operation) => respond(operation.procedure, await operation.run(repo))),
  );
  const total = performance.now() - start;
  const busy = performance.eventLoopUtilization(utilization).active;
  delay.disable();
  return { total, busy, stall: delay.max / 1e6 };
}

describe.each(perfRepoPaths())("%s", (path) => {
  it("git commands", async () => {
    const repo = await openRepo(path);
    const status = await getStatus(repo);
    const inView = status.changes.unstaged
      .filter((file) => file.status !== "untracked")
      .slice(0, FILES_IN_VIEW);
    const all = operations(git(path, "rev-parse", "HEAD"), inView);

    const rows = [];
    for (const operation of all) {
      const timings: Timing[] = [];
      for (let run = 0; run <= RUNS; run++) {
        const timing = await measure(repo, operation);
        if (run > 0) timings.push(timing);
      }
      const med = (key: keyof Timing) => median(timings.map((timing) => timing[key]));
      const blocking = med("spawn") + med("parse") + med("validate") + med("encode");
      rows.push({
        operation: operation.name,
        total: ms(med("total")),
        p95: ms(
          percentile(
            timings.map((timing) => timing.total),
            0.95,
          ),
        ),
        git: ms(med("git")),
        spawn: ms(med("spawn")),
        parse: ms(med("parse")),
        validate: ms(med("validate")),
        encode: ms(med("encode")),
        blocking: ms(blocking),
        "decode (ui)": ms(med("decode")),
        message: bytes(med("bytes")),
      });
    }
    printTable(`${describeRepo(path)} (median ms of ${RUNS} runs)`, rows);

    const changes = [
      { change: "file edited or staged", refetched: ["uncommitted"] },
      { change: "commit or checkout", refetched: ["uncommitted", "refs"] },
    ];
    const refreshRows = [];
    for (const { change, refetched } of changes) {
      const refetchedOps = all.filter(
        (operation) => operation.refetchedOn && refetched.includes(operation.refetchedOn),
      );
      const refreshes = [];
      for (let run = 0; run <= RUNS; run++) {
        const refresh = await measureRefresh(repo, refetchedOps);
        if (run > 0) refreshes.push(refresh);
      }
      refreshRows.push({
        change,
        refetches: refetchedOps.map((operation) => operation.name).join(", "),
        total: ms(median(refreshes.map((r) => r.total))),
        busy: ms(median(refreshes.map((r) => r.busy))),
        "longest stall": ms(median(refreshes.map((r) => r.stall))),
        "worst stall": ms(Math.max(...refreshes.map((r) => r.stall))),
      });
    }
    printTable("Refetch after a change on disk (median ms)", refreshRows);
  });
});
