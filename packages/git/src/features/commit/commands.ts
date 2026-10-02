import type { Repo } from "../../core/repo";

/**
 * Commits what's staged; with `amend`, replaces the last commit with it instead. An amend may leave
 * the commit empty, so an empty one can still be reworded.
 */
export async function createCommit(
  repo: Repo,
  message: string,
  options: { amend?: boolean } = {},
): Promise<void> {
  const args = ["commit", ...(options.amend ? ["--amend", "--allow-empty"] : []), "-F", "-"];
  await repo.write(args, { stdin: message });
}

/** A commit's message as it was written, unlike the log's subject, which joins its first lines. */
export async function getCommitMessage(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<string> {
  const message = await repo.read(["log", "--max-count=1", "--format=%B", sha, "--"], { signal });
  return message.trimEnd();
}

/** A remote branch that has the commit, e.g. `origin/main`, if it's been pushed anywhere. */
export async function getPushedTo(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<string | null> {
  const output = await repo.read(
    ["for-each-ref", "--contains", sha, "--format=%(symref)%00%(refname:lstrip=2)", "refs/remotes"],
    { signal },
  );
  // Skips symbolic refs like `origin/HEAD`, which only point at another remote branch.
  const branch = output.split("\n").find((line) => line.startsWith("\0"));
  return branch?.slice(1) ?? null;
}
