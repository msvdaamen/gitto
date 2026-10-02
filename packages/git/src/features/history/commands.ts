import { createHash } from "node:crypto";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit, LogPage } from "./schema";

/** `limit` commits of the history, after the first `skip`. */
export async function getLog(
  repo: Repo,
  page: { limit: number; skip: number },
  signal?: AbortSignal,
): Promise<LogPage> {
  // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
  const refs = ["--branches", "--remotes", "--tags"];
  const log = (revisions: string[]) =>
    repo.read(
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
  let output: string;
  try {
    // HEAD is included for detached checkouts.
    output = await log([...refs, "HEAD"]);
  } catch (error) {
    // On a branch without commits yet, HEAD doesn't name one. Tried again without it, rather than
    // checking first: that would start another git process for every log.
    if (!(error instanceof GitError) || !/bad revision 'HEAD'/.test(error.stderr)) throw error;
    output = await log(refs);
  }
  // Everything shown comes from git's output, so the same output means the same commits.
  const version = createHash("sha1").update(output).digest("hex");
  return { commits: parseLog(output), version };
}

export async function getCommit(repo: Repo, sha: string, signal?: AbortSignal): Promise<Commit> {
  const args = ["log", "-z", LOG_FORMAT, "--decorate=full", "--max-count=1", sha, "--"];
  const [commit] = parseLog(await repo.read(args, { signal }));
  // `sha` names a tree or blob, say, rather than a commit.
  if (!commit) throw new GitError(`${sha} is not a commit.`, args, 0, "");
  return commit;
}
