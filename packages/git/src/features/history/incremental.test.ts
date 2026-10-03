import { execFileSync } from "node:child_process";

import { describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { createRepo, git, repos } from "../../test/fixtures";
import { getLog, getVersionedLog } from "./commands";
import { indexed } from "./incremental";
import type { Commit } from "./schema";

/** Small, so changes reach past the end of the page. */
const page = { limit: 8, skip: 0 };

/** Runs git in `path` with commits dated `seconds` (since the epoch), and returns its output. */
function gitAt(path: string, seconds: number, ...args: string[]): string {
  const date = `@${seconds} +0000`;
  return execFileSync("git", args, {
    cwd: path,
    encoding: "utf8",
    env: { ...process.env, GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date },
  }).trim();
}

/** A commit made at `seconds`, with `parents`, that nothing points at yet. */
function commit(path: string, seconds: number, message: string, ...parents: string[]): string {
  const tree = git(path, "hash-object", "-t", "tree", "/dev/null");
  return gitAt(
    path,
    seconds,
    "commit-tree",
    tree,
    ...parents.flatMap((p) => ["-p", p]),
    "-m",
    message,
  );
}

/** A page of the log, as the UI has it. */
type Seen = { version: string; commits: Commit[] };

/**
 * Reads the log as the UI does, from the page it has, and checks it's the page git reads whole.
 * Returns the page, and whether it had to be read whole too.
 */
async function readSince(repo: Repo, seen: Seen | undefined) {
  let readWhole = false;
  const recording: Repo = {
    ...repo,
    read: (args, options) => {
      if (args.includes("--date-order")) readWhole = true;
      return repo.read(args, options);
    },
  };
  const log = await getVersionedLog(recording, page, seen?.version, undefined, {
    readChangesAfterMs: 0,
  });
  const now = "unchanged" in log ? seen! : log;
  expect(now.commits).toEqual(await getLog(repo, page));
  // As the index has time to load or catch up between refreshes in the app.
  await indexed(repo);
  return { ...now, readWhole };
}

describe("reading the log from what changed", () => {
  it("reads what a change added, and the whole log when it can't", async () => {
    const path = createRepo("incremental");
    const repo = await repos.open("incremental");
    const main = (seconds: number, message: string) => {
      const sha = commit(path, seconds, message, git(path, "rev-parse", "main"));
      git(path, "update-ref", "refs/heads/main", sha);
      return sha;
    };
    let tip = commit(path, 1000, "m0");
    git(path, "update-ref", "refs/heads/main", tip);
    for (let i = 1; i < 12; i++) tip = main(1000 + i, `m${i}`);
    const fork = git(path, "rev-parse", "main~6");
    let side = fork;
    for (let i = 0; i < 3; i++) side = commit(path, 2000 + i, `s${i}`, side);
    git(path, "update-ref", "refs/heads/side", side);
    git(path, "tag", "v1", "main~3");
    gitAt(path, 1500, "tag", "-a", "-m", "release", "v2", "main~1");

    let seen = await readSince(repo, undefined);
    const steps: [change: string, expected: "changes" | "whole", run: () => void][] = [
      ["a commit on top", "changes", () => main(3000, "m12")],
      [
        "commits on a branch below the top",
        "changes",
        () => {
          for (let i = 0; i < 3; i++) side = commit(path, 3001 + i, `s${3 + i}`, side);
          git(path, "update-ref", "refs/heads/side", side);
        },
      ],
      ["a branch at a commit there was", "changes", () => git(path, "branch", "other", "main~2")],
      [
        "an annotated tag",
        "changes",
        () => gitAt(path, 3100, "tag", "-a", "-m", "tag", "v3", "main~1"),
      ],
      ["a merged branch deleted", "changes", () => git(path, "branch", "-D", "other")],
      ["switching branches", "changes", () => git(path, "symbolic-ref", "HEAD", "refs/heads/side")],
      ["detaching HEAD", "changes", () => git(path, "update-ref", "--no-deref", "HEAD", side)],
      [
        "a merge",
        "changes",
        () => {
          git(path, "symbolic-ref", "HEAD", "refs/heads/main");
          tip = commit(path, 4000, "merge", git(path, "rev-parse", "main"), side);
          git(path, "update-ref", "refs/heads/main", tip);
        },
      ],
      [
        "an amend, which loses a commit",
        "whole",
        () => {
          const [parent, merged] = [git(path, "rev-parse", "main^1"), side];
          git(path, "update-ref", "refs/heads/main", commit(path, 4001, "amended", parent, merged));
        },
      ],
      ["a commit older than the newest", "whole", () => main(500, "skewed")],
      [
        "commits of the same second on two branches",
        "whole",
        () => {
          git(path, "update-ref", "refs/heads/side", commit(path, 5000, "s-same", side));
          main(5000, "m-same");
        },
      ],
      [
        "a fetch bringing a remote branch",
        "changes",
        () => {
          let remote = git(path, "rev-parse", "main~4");
          for (let i = 0; i < 4; i++) remote = commit(path, 6000 + i, `r${i}`, remote);
          git(path, "update-ref", "refs/remotes/origin/feature", remote);
        },
      ],
      [
        "a branch with commits of its own deleted",
        "whole",
        () => git(path, "update-ref", "-d", "refs/remotes/origin/feature"),
      ],
      [
        // Older than the page's commits, so not on top of it.
        "commits on a branch whose clock was skewed",
        "whole",
        () => {
          // Eight commits dated well before the one they're on: walking by date from `behind`,
          // git reaches `skewed` only after giving up.
          const skewed = commit(path, 8000, "skewed", git(path, "rev-parse", "main"));
          let behind = skewed;
          for (let i = 0; i < 8; i++) behind = commit(path, 100 + i, `behind${i}`, behind);
          git(path, "update-ref", "refs/heads/behind", behind);
        },
      ],
      [
        // Git takes the skewed commit (and what's below it) for new too.
        "a commit on the skewed commit",
        "changes",
        () => {
          const skewed = git(path, "rev-parse", "behind~8");
          git(path, "update-ref", "refs/heads/onto-skewed", commit(path, 9000, "onto", skewed));
        },
      ],
      [
        "more new commits than a page",
        "whole",
        () => {
          for (let i = 0; i < page.limit; i++) main(7000 + i, `many${i}`);
        },
      ],
    ];
    for (const [change, expected, run] of steps) {
      run();
      // oxlint-disable-next-line no-await-in-loop -- each change follows the one before.
      seen = await readSince(repo, seen);
      expect(seen.readWhole ? "whole" : "changes", change).toBe(expected);
    }
  });

  it("reads the log git would after any changes", { timeout: 60_000 }, async () => {
    const path = createRepo("incremental-random");
    const repo = await repos.open("incremental-random");
    const random = seeded(7);
    const pick = <T>(items: T[]) => items[Math.floor(random() * items.length)]!;
    const branches = () =>
      git(path, "for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes")
        .split("\n")
        .filter(Boolean);
    const commits = () => git(path, "rev-list", "--all", "--max-count=40").split("\n");
    let now = 10_000;
    // For names, which the clock going back would repeat.
    let made = 0;
    // Mostly later, sometimes the same second, now and then behind (a skewed clock).
    const when = () => (now += pick([1, 1, 1, 0, 0, 5, -50]));

    git(path, "update-ref", "refs/heads/main", commit(path, now, "root"));
    git(path, "symbolic-ref", "HEAD", "refs/heads/main");
    const changes: (() => void)[] = [
      // A commit on a branch.
      () => {
        const ref = pick(branches());
        git(
          path,
          "update-ref",
          ref,
          commit(path, when(), `c${made++}`, git(path, "rev-parse", ref)),
        );
      },
      () => {
        const ref = pick(branches());
        git(
          path,
          "update-ref",
          ref,
          commit(path, when(), `c${made++}`, git(path, "rev-parse", ref)),
        );
      },
      // A branch or remote branch made at a commit.
      () =>
        git(
          path,
          "update-ref",
          `${pick(["refs/heads", "refs/remotes/origin"])}/b${made++}`,
          pick(commits()),
        ),
      // A merge of two branches into the first.
      () => {
        const [a, b] = [pick(branches()), pick(branches())];
        const [shaA, shaB] = [git(path, "rev-parse", a), git(path, "rev-parse", b)];
        if (shaA !== shaB)
          git(path, "update-ref", a, commit(path, when(), `merge${made++}`, shaA, shaB));
      },
      // A branch deleted, but for the one checked out, and main, so there's always one.
      () => {
        const ref = pick(branches());
        const current = git(path, "rev-parse", "--symbolic-full-name", "HEAD");
        if (ref !== current && ref !== "refs/heads/main") git(path, "update-ref", "-d", ref);
      },
      // A branch reset to its parent.
      () => {
        const ref = pick(branches());
        const parent = git(path, "rev-list", "--max-count=1", "--skip=1", ref);
        if (parent) git(path, "update-ref", ref, parent);
      },
      () =>
        git(
          path,
          "symbolic-ref",
          "HEAD",
          pick(branches().filter((r) => r.startsWith("refs/heads/"))),
        ),
      () => git(path, "update-ref", "--no-deref", "HEAD", pick(commits())),
      () => git(path, "tag", `t${made++}`, pick(commits())),
      () => gitAt(path, when(), "tag", "-a", "-m", "t", `a${made++}`, pick(commits())),
    ];

    let seen = await readSince(repo, undefined);
    let fromChanges = 0;
    const STEPS = 150;
    for (let step = 0; step < STEPS; step++) {
      pick(changes)();
      const before = seen.version;
      // oxlint-disable-next-line no-await-in-loop -- each change follows the one before.
      seen = await readSince(repo, seen);
      if (seen.version !== before && !seen.readWhole) fromChanges++;
    }
    // The point: plenty of these didn't need the whole log.
    expect(fromChanges).toBeGreaterThan(STEPS / 3);
  });
});

/** Random numbers from 0 to 1, the same ones for the same seed (mulberry32). */
function seeded(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}
