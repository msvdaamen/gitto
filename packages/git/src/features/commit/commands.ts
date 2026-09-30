import type { Repo } from "../../core/repo";

export async function createCommit(repo: Repo, message: string): Promise<void> {
  await repo.write(["commit", "-F", "-"], { stdin: message });
}
