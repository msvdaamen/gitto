import type { Repo } from "../../core/repo";

export async function stage(repo: Repo, paths: string[]): Promise<void> {
  await repo.write(["add", "--", ...paths]);
}

export async function unstage(repo: Repo, paths: string[]): Promise<void> {
  // `restore --staged` resets to HEAD, so it can't run before the first commit.
  const args = (await repo.hasHead())
    ? ["restore", "--staged", "--", ...paths]
    : ["rm", "--cached", "-r", "-q", "--", ...paths];
  await repo.write(args);
}
