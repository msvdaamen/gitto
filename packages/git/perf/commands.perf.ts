// Times every read the UI makes on real repositories. Each is split into the git process, which
// runs alongside the main process, and the work that blocks the main process while it runs:
// starting git, parsing its output, checking it against the contract and encoding it for the
// renderer. `decode` is the renderer's share: turning the message back into objects.
//
//   GITTO_PERF_REPOS=~/code/vscode,~/code/linux pnpm perf
// Measurements run one at a time on purpose, so they don't slow each other down.
/* oxlint-disable no-await-in-loop */
import { monitorEventLoopDelay } from "node:perf_hooks";

import { StandardRPCJsonSerializer, StandardRPCSerializer } from "@orpc/client/standard";
import type { AnyContractProcedure } from "@orpc/contract";
import { describe, it } from "vitest";

import type { Repo } from "../src/core/repo";
import { getCommitFiles } from "../src/features/diff/commands";
import { DiffContract } from "../src/features/diff/contract";
import { getCommit, getVersionedLog } from "../src/features/history/commands";
import { HistoryContract } from "../src/features/history/contract";
import { listRefs } from "../src/features/refs/commands";
import { RefsContract } from "../src/features/refs/contract";
import { listStashes } from "../src/features/stash/commands";
import { StashContract } from "../src/features/stash/contract";
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
  /** Runs it; with `since`, the version of what the caller has already, as the UI passes it. */
  run: (repo: Repo, since?: string) => Promise<unknown>;
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

/** `result`, or `{ unchanged: true }` if its version is `since`, as the handlers answer. */
function unlessUnchanged<T extends { version: string }>(result: T, since?: string) {
  return result.version === since ? { unchanged: true as const } : result;
}

function operations(head: string): Operation[] {
  return [
    {
      name: "status",
      run: async (repo, since) => unlessUnchanged(await getStatus(repo), since),
      procedure: StatusContract.get,
      refetchedOn: "uncommitted",
    },
    {
      name: "log 200",
      run: (repo, since) => getVersionedLog(repo, { limit: 200, skip: 0 }, since),
      procedure: HistoryContract.log,
      refetchedOn: "refs",
    },
    {
      name: "log 200, no commit-graph",
      run: (repo) => withoutCommitGraph(() => getVersionedLog(repo, { limit: 200, skip: 0 })),
      procedure: HistoryContract.log,
    },
    {
      name: "log 1000",
      run: (repo) => getVersionedLog(repo, { limit: 1000, skip: 0 }),
      procedure: HistoryContract.log,
    },
    {
      name: "log 200, skip 10000",
      run: (repo) => getVersionedLog(repo, { limit: 200, skip: 10_000 }),
      procedure: HistoryContract.log,
    },
    {
      name: "refs",
      run: async (repo, since) => unlessUnchanged(await listRefs(repo), since),
      procedure: RefsContract.list,
      refetchedOn: "refs",
    },
    {
      name: "stashes",
      run: (repo) => listStashes(repo),
      procedure: StashContract.list,
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

/** The version of what each operation returns now, for those that have one. */
async function versions(repo: Repo, refetched: Operation[]) {
  const results = await Promise.all(refetched.map((operation) => operation.run(repo)));
  return new Map(
    refetched.map((operation, i) => {
      const result = results[i] as { version?: string };
      return [operation, result.version] as const;
    }),
  );
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
 * Everything a change on disk refetches, at once, like the UI does: with `since`, the versions of
 * what it has already, by operation. `busy` is how much of that time the main process was occupied
 * (and so couldn't handle input); `stall` its longest block.
 */
async function measureRefresh(
  repo: Repo,
  refetched: Operation[],
  since?: Map<Operation, string | undefined>,
) {
  const delay = monitorEventLoopDelay({ resolution: 1 });
  delay.enable();
  const utilization = performance.eventLoopUtilization();
  const start = performance.now();
  await Promise.all(
    refetched.map(async (operation) =>
      respond(operation.procedure, await operation.run(repo, since?.get(operation))),
    ),
  );
  const total = performance.now() - start;
  const busy = performance.eventLoopUtilization(utilization).active;
  delay.disable();
  return { total, busy, stall: delay.max / 1e6 };
}

describe.each(perfRepoPaths())("%s", (path) => {
  it("git commands", async () => {
    const repo = await openRepo(path);
    const all = operations(git(path, "rev-parse", "HEAD"));

    const rows = [];
    for (const operation of all) {
      const timings: Timing[] = [];
      for (let run = 0; run <= RUNS; run++) {
        const timing = await measure(repo, operation);
        if (run > 0) timings.push(timing);
      }
      const med = (key: keyof Timing) => median(timings.map((timing) => timing[key]));
      const mainThread = med("spawn") + med("parse") + med("validate") + med("encode");
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
        "main thread": ms(mainThread),
        "decode (ui)": ms(med("decode")),
        message: bytes(med("bytes")),
      });
    }
    printTable(`${describeRepo(path)} (median ms of ${RUNS} runs)`, rows);

    const changes = [
      { change: "file edited or staged", refetched: ["uncommitted"] },
      { change: "commit or checkout", refetched: ["uncommitted", "refs"] },
      // What the UI has is sent back as its version: e.g. after a change made in Gitto, which it
      // refetches right away and once more when the watcher reports it.
      { change: "nothing changed (refs)", refetched: ["uncommitted", "refs"], unchanged: true },
    ];
    const refreshRows = [];
    for (const { change, refetched, unchanged } of changes) {
      const refetchedOps = all.filter(
        (operation) => operation.refetchedOn && refetched.includes(operation.refetchedOn),
      );
      const since = unchanged ? await versions(repo, refetchedOps) : undefined;
      const refreshes = [];
      for (let run = 0; run <= RUNS; run++) {
        const refresh = await measureRefresh(repo, refetchedOps, since);
        if (run > 0) refreshes.push(refresh);
      }
      refreshRows.push({
        change,
        refetches: refetchedOps.map((operation) => operation.name).join(", "),
        total: ms(median(refreshes.map((r) => r.total))),
        "main thread busy": ms(median(refreshes.map((r) => r.busy))),
        "longest stall": ms(median(refreshes.map((r) => r.stall))),
        "worst stall": ms(Math.max(...refreshes.map((r) => r.stall))),
      });
    }
    printTable("Refetch after a change on disk (median ms)", refreshRows);
  });
});
