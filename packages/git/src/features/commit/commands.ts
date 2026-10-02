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
