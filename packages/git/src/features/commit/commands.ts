import type { Repo } from "../../core/repo";

/** Commits what's staged; with `amend`, replaces the last commit with it instead. */
export async function createCommit(
  repo: Repo,
  message: string,
  options: { amend?: boolean } = {},
): Promise<void> {
  const args = ["commit", ...(options.amend ? ["--amend"] : []), "-F", "-"];
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
