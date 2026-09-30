import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit } from "./schema";

export async function getLog(
  repo: Repo,
  page: { limit: number; skip: number },
  signal?: AbortSignal,
): Promise<Commit[]> {
  // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
  // HEAD is included for detached checkouts, unless the branch has no commits yet.
  const revisions = [
    "--branches",
    "--remotes",
    "--tags",
    ...((await repo.hasHead()) ? ["HEAD"] : []),
  ];
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
  return parseLog(output);
}

export async function getCommit(repo: Repo, sha: string, signal?: AbortSignal): Promise<Commit> {
  const args = ["log", "-z", LOG_FORMAT, "--decorate=full", "--max-count=1", sha, "--"];
  const [commit] = parseLog(await repo.read(args, { signal }));
  // `sha` names a tree or blob, say, rather than a commit.
  if (!commit) throw new GitError(`${sha} is not a commit.`, args, 0, "");
  return commit;
}
