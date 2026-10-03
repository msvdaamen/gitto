import type { Repo } from "../../core/repo";

/**
 * Creates the branch `name` at HEAD and switches to it. HEAD stays on the same commit, so every
 * uncommitted change, staged or not, comes along to the new branch as it is. It doesn't track the
 * branch it was made from, so pulling it doesn't pull that one's commits into it. Git refuses a
 * name that's taken or not a valid branch name, and to switch in the middle of a merge or rebase,
 * saying so.
 */
export async function createBranch(repo: Repo, name: string): Promise<void> {
  // Without the hint pointing to `git help check-ref-format` after an invalid name.
  await repo.write(["switch", "--no-track", "--create", name], {
    config: ["advice.refSyntax=false"],
  });
}
