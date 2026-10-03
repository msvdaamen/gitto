import { createHash } from "node:crypto";

import { GitError } from "../../core/errors";
import { resolveRef, type Repo } from "../../core/repo";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit, Log } from "./schema";

type Page = { limit: number; skip: number };

/**
 * What the log starts from, besides HEAD: every branch, remote branch and tag (and the replace
 * refs, which change what a commit's parents are), with the checked-out branch marked, as that's
 * in the log's decorations.
 */
const LOG_REFS_ARGS = [
  "for-each-ref",
  "--format=%(objectname) %(HEAD) %(refname)",
  "refs/heads",
  "refs/remotes",
  "refs/tags",
  "refs/replace",
];

/**
 * The page of the log `getLog` reads, with a version of it; `{ unchanged: true }` instead when
 * that's `since`. Commits never change, so neither does the log until a ref it starts from does,
 * or HEAD: the version comes from those, so an unchanged log isn't read again. That takes long in a
 * big repository, as it sorts the history of every branch and tag (0.35s for vscode's 5,800).
 */
export async function getVersionedLog(
  repo: Repo,
  page: Page,
  since?: string,
  signal?: AbortSignal,
): Promise<Log | { unchanged: true }> {
  const [head, refs] = await Promise.all([
    // Any failure is taken for no HEAD, as `Repo.hasHead` does.
    resolveRef((args) => repo.read(args, { signal }), "HEAD").catch(() => null),
    repo.read(LOG_REFS_ARGS, { signal }),
  ]);
  const version = createHash("sha1")
    .update(`${page.limit} ${page.skip} ${head}\0${refs}`)
    .digest("hex");
  if (version === since) return { unchanged: true };
  // Read after the refs, so it's never older than the version says: a ref that moves in between
  // only makes the next refetch read it again.
  return { commits: await readLog(repo, page, head !== null, signal), version };
}

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
