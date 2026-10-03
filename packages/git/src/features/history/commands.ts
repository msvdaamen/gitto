import { createHash } from "node:crypto";

import { GitError } from "../../core/errors";
import { resolveRef, type Repo } from "../../core/repo";
import {
  fullReadMs,
  LOG_REFS_ARGS,
  parseLogRefs,
  readLogSince,
  rememberedLog,
  rememberLog,
  type Page,
} from "./incremental";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit, Log } from "./schema";

/**
 * Below this, a repository's log is read whole every time: reading only what changed takes a few
 * git commands of its own, and can't always be done.
 */
const READ_CHANGES_AFTER_MS = 100;

/**
 * The page of the log `getLog` reads, with a version of it; `{ unchanged: true }` instead when
 * that's `since`. Commits never change, so neither does the log until a ref it starts from does,
 * or HEAD: the version comes from those, so an unchanged log isn't read again. That takes long in a
 * big repository, as it sorts the history of every branch and tag (0.35s for vscode's 5,800); there,
 * a log that did change is read from what changed since `since`, when it can be (see
 * `readLogSince`). `readChangesAfterMs` is how slow reading it whole must have been for that.
 */
export async function getVersionedLog(
  repo: Repo,
  page: Page,
  since?: string,
  signal?: AbortSignal,
  { readChangesAfterMs = READ_CHANGES_AFTER_MS } = {},
): Promise<Log | { unchanged: true }> {
  const [head, refsOutput] = await Promise.all([
    // Any failure is taken for no HEAD, as `Repo.hasHead` does.
    resolveRef((args) => repo.read(args, { signal }), "HEAD").catch(() => null),
    repo.read(LOG_REFS_ARGS, { signal }),
  ]);
  const version = createHash("sha1")
    .update(`${page.limit} ${page.skip} ${head}\0${refsOutput}`)
    .digest("hex");
  if (version === since) return { unchanged: true };

  const refs = parseLogRefs(refsOutput);
  const previous = rememberedLog(repo, since);
  // Read after the refs, so it's never older than the version says: a ref that moves in between
  // only makes the next refetch read it again.
  let commits =
    previous && fullReadMs(repo) >= readChangesAfterMs
      ? await readLogSince(repo, previous, head, refs, signal)
      : undefined;
  let readMs: number | undefined;
  if (!commits) {
    const start = performance.now();
    // And the commit after the page, which reading the next one from changes needs.
    commits = await readLog(repo, { ...page, limit: page.limit + 1 }, head !== null, signal);
    readMs = performance.now() - start;
  }
  rememberLog(repo, { version, page, head, refs, commits }, readMs);
  return { commits: commits.slice(0, page.limit), version };
}

/** A page of the history of all branches, remotes and tags (see `HistoryContract.log`). */
export async function getLog(repo: Repo, page: Page, signal?: AbortSignal): Promise<Commit[]> {
  return readLog(repo, page, await repo.hasHead(), signal);
}

async function readLog(
  repo: Repo,
  page: Page,
  hasHead: boolean,
  signal?: AbortSignal,
): Promise<Commit[]> {
  // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
  // HEAD is included for detached checkouts, unless the branch has no commits yet.
  const revisions = ["--branches", "--remotes", "--tags", ...(hasHead ? ["HEAD"] : [])];
  const output = await repo.read(
    [
      "log",
      "-z",
      LOG_FORMAT,
      "--date-order",
      "--decorate=full",
      `--max-count=${page.limit}`,
      `--skip=${page.skip}`,
      ...revisions,
      "--",
    ],
    { signal },
  );
  // After the log, not alongside it: the first write can take seconds, and would slow it down.
  void repo.updateCommitGraph();
  return parseLog(output);
}

export async function getCommit(repo: Repo, sha: string, signal?: AbortSignal): Promise<Commit> {
  const args = ["log", "-z", LOG_FORMAT, "--decorate=full", "--max-count=1", sha, "--"];
  const [commit] = parseLog(await repo.read(args, { signal }));
  // `sha` names a tree or blob, say, rather than a commit.
  if (!commit) throw new GitError(`${sha} is not a commit.`, args, 0, "");
  return commit;
}
