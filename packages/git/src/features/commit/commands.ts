import { GitError, HeadMovedError } from "../../core/errors";
import type { Repo } from "../../core/repo";

/**
 * Commits what's staged. With `amend`, HEAD's SHA, replaces that commit with it instead, as long as
 * HEAD is still that commit. An amend may leave the commit empty, so an empty one can be reworded.
 */
export async function createCommit(
  repo: Repo,
  message: string,
  options: { amend?: string } = {},
): Promise<void> {
  const { amend } = options;
  // One write, so no other commit can land between checking HEAD and amending it.
  await repo.writeTogether(async (run) => {
    if (amend) {
      const head = await run(["rev-parse", "--verify", "--quiet", "HEAD"]).then(
        (sha) => sha.trim(),
        unbornHead,
      );
      if (head !== amend) throw new HeadMovedError(amend);
    }
    const args = ["commit", ...(amend ? ["--amend", "--allow-empty"] : []), "-F", "-"];
    await run(args, { stdin: message });
  });
}

/** `rev-parse --verify --quiet` exits 1 without a word when HEAD has no commit; rethrows the rest. */
function unbornHead(error: unknown): string {
  if (error instanceof GitError && error.exitCode === 1 && !error.stderr.trim()) return "";
  throw error;
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

/**
 * The remote branch `git push` would update that already has the commit, e.g. `origin/feature`.
 * Not a branch the checked-out one was only created from, e.g. `origin/main`, which amending
 * doesn't rewrite; and none with HEAD detached, as amending then rewrites no branch.
 */
export async function getPushedTo(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<string | null> {
  // Both exit with an error when there's nothing to find, which only means there's no warning.
  const lookUp = (args: string[]) =>
    repo.read(args, { signal }).then(
      (output) => output.trim(),
      () => "",
    );
  const branch = await lookUp(["symbolic-ref", "--quiet", "--short", "HEAD"]);
  if (!branch) return null;
  // Where the repository's config pushes it. When that can't say, e.g. because its upstream has
  // another name, the branch of the same name on any remote, which `git push <remote>` updates.
  const push = await lookUp(["rev-parse", "--symbolic-full-name", "@{push}"]);
  const pushedTo = await repo.read(
    [
      "for-each-ref",
      "--count=1",
      "--contains",
      sha,
      "--format=%(refname:lstrip=2)",
      push || `refs/remotes/*/${branch}`,
    ],
    { signal },
  );
  return pushedTo.trim() || null;
}
