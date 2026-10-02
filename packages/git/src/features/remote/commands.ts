import { GitError, NoUpstreamError, PullConflictError } from "../../core/errors";
import type { Repo } from "../../core/repo";

/**
 * Fetches the current branch's upstream and merges it in, or rebases onto it if the user's config
 * says to. Conflicts are left in the working tree, to resolve and commit.
 */
export async function pull(repo: Repo): Promise<void> {
  const branch = await currentBranch(repo);
  if (!branch) throw new NoUpstreamError("Check out a branch to pull into it.");
  // Read from the config rather than `@{upstream}`, which also needs the remote-tracking branch:
  // that's missing before the first fetch, and the pull fetches it.
  const upstream = (
    await repo.read(["for-each-ref", "--format=%(upstream:short)", `refs/heads/${branch}`])
  ).trim();
  if (!upstream) {
    throw new NoUpstreamError(
      `${branch} doesn't track a remote branch, so there's nothing to pull.`,
    );
  }

  // Git refuses to pull a branch that diverged from its upstream until told whether to merge or
  // rebase. Merge, like git did by default before 2.27, unless the user's config says to rebase.
  const configured =
    (await config(repo, `branch.${branch}.rebase`)) ?? (await config(repo, "pull.rebase"));
  // Quiet, so the fetch's progress doesn't take the place of the reason in an error.
  const args = ["pull", "--quiet", ...(configured === null ? ["--no-rebase"] : [])];
  // Git won't pull over unresolved conflicts, and says so; those aren't the pull's.
  const hadConflicts = await hasConflicts(repo);

  try {
    await repo.write(args);
  } catch (error) {
    if (error instanceof GitError && !hadConflicts && (await hasConflicts(repo))) {
      const next = (await isRebasing(repo)) ? "continue the rebase" : "commit the merge";
      throw new PullConflictError(
        `Pulling ${upstream} caused conflicts. Resolve them, then ${next}.`,
        error.args,
        error.exitCode,
        error.stderr,
      );
    }
    throw error;
  }
}

/** The checked-out branch's name; `null` when HEAD is detached. */
async function currentBranch(repo: Repo): Promise<string | null> {
  try {
    return (await repo.read(["symbolic-ref", "--quiet", "--short", "HEAD"])).trim();
  } catch (error) {
    if (error instanceof GitError && error.exitCode === 1) return null;
    throw error;
  }
}

/** A config value, wherever it's set; `null` when it isn't. */
async function config(repo: Repo, key: string): Promise<string | null> {
  try {
    return (await repo.read(["config", "--get", key])).trim();
  } catch (error) {
    if (error instanceof GitError && error.exitCode === 1) return null;
    throw error;
  }
}

async function hasConflicts(repo: Repo): Promise<boolean> {
  return (await repo.read(["ls-files", "--unmerged"])) !== "";
}

/** Whether a rebase stopped partway, e.g. at a conflict. */
function isRebasing(repo: Repo): Promise<boolean> {
  return repo.read(["rev-parse", "--quiet", "--verify", "REBASE_HEAD"]).then(
    () => true,
    () => false,
  );
}
