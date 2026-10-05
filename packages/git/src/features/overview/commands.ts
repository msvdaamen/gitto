import { stat } from "node:fs/promises";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { parseReflog, pickRemote, REFLOG_FORMAT } from "./parse";
import type { Activity, Overview } from "./schema";

/** How many of the latest entries of HEAD's reflog are read, of which some aren't shown. */
const REFLOG_ENTRIES = 50;

/** How many of them are sent: the home page only shows the latest few of all repositories. */
const MAX_ACTIVITY = 10;

/** What the home page shows of a repository, beyond its status; all of it read, none written. */
export async function getOverview(repo: Repo, signal?: AbortSignal): Promise<Overview> {
  const hasHead = await repo.hasHead();
  const [remote, fetchedAt, committedAt, activity] = await Promise.all([
    readRemote(repo, signal),
    readFetchedAt(repo, signal),
    hasHead ? readCommittedAt(repo, signal) : null,
    hasHead ? readActivity(repo, signal) : [],
  ]);
  return { remote, fetchedAt, committedAt, activity };
}

async function readRemote(repo: Repo, signal?: AbortSignal): Promise<Overview["remote"]> {
  try {
    return pickRemote(
      await repo.read(["config", "-z", "--get-regexp", String.raw`^remote\..+\.url$`], { signal }),
    );
  } catch (error) {
    // There are no remotes.
    if (error instanceof GitError && error.exitCode === 1) return null;
    throw error;
  }
}

/** When FETCH_HEAD, which every fetch writes, was last written; `null` if it never was. */
async function readFetchedAt(repo: Repo, signal?: AbortSignal): Promise<number | null> {
  // In the worktree's own git directory.
  const path = await repo.read(
    ["rev-parse", "--path-format=absolute", "--git-path", "FETCH_HEAD"],
    {
      signal,
    },
  );
  return stat(path.trim()).then(
    (stats) => stats.mtimeMs,
    () => null,
  );
}

async function readCommittedAt(repo: Repo, signal?: AbortSignal): Promise<number> {
  return Number(await repo.read(["log", "-1", "--format=%ct", "HEAD"], { signal })) * 1000;
}

async function readActivity(repo: Repo, signal?: AbortSignal): Promise<Activity[]> {
  const output = await repo.read(
    ["log", "-g", "-z", "--date=unix", REFLOG_FORMAT, `--max-count=${REFLOG_ENTRIES}`, "HEAD"],
    { signal },
  );
  return parseReflog(output).slice(0, MAX_ACTIVITY);
}
