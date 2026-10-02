import { HeadMovedError } from "../../core/errors";
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
  if (options.amend) {
    // Through the write queue, so a commit still waiting to run is seen.
    const head = await repo.write(["rev-parse", "--verify", "HEAD"]).then(
      (sha) => sha.trim(),
      () => "",
    );
    if (head !== options.amend) throw new HeadMovedError(options.amend);
  }
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

/**
 * The remote branch `git push` would update that already has the commit, e.g. `origin/feature`
 * for `feature`: one with the checked-out branch's name, as `push.default` picks by default. Not a
 * branch it was only created from, e.g. `origin/main`, which amending doesn't rewrite.
 */
export async function getPushedTo(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<string | null> {
  // With HEAD detached, amending rewrites no branch.
  const head = await repo.read(["symbolic-ref", "--quiet", "--short", "HEAD"], { signal }).then(
    (name) => name.trim(),
    () => "",
  );
  if (!head) return null;
  const output = await repo.read(
    ["for-each-ref", "--contains", sha, "--format=%(refname:lstrip=2)", "refs/remotes"],
    { signal },
  );
  // `<remote>/<branch>`; a remote's name has no slash in practice.
  const branch = output.split("\n").find((ref) => ref.slice(ref.indexOf("/") + 1) === head);
  return branch ?? null;
}
