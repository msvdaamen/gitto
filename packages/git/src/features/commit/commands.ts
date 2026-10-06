import { HeadMovedError } from "../../core/errors";
import { currentBranch, resolveRef, type Repo } from "../../core/repo";

/**
 * Commits what's staged, with `message` as it's written. With `amend`, HEAD's SHA, replaces that
 * commit with it instead, as long as HEAD is still that commit. An amend may leave the commit empty,
 * so an empty one can be reworded.
 */
export async function createCommit(
  repo: Repo,
  message: string,
  options: { amend?: string } = {},
): Promise<void> {
  const { amend } = options;
  // One write, so no other commit of Gitto's can land between checking HEAD and amending it.
  await repo.exclusive(async (run) => {
    if (amend && (await resolveRef(run, "HEAD")) !== amend) throw new HeadMovedError();
    // An amended message is kept exactly as sent, whatever `commit.cleanup` says, as what wasn't
    // edited of it is as it was written.
    const args = [
      "commit",
      ...(amend ? ["--amend", "--allow-empty", "--cleanup=verbatim"] : []),
      "-F",
      "-",
    ];
    // A new commit's message is cleaned up by git, but for the spaces it'd leave before the subject.
    await run(args, { stdin: amend ? message : message.trim() });
  });
}

/** A commit's message exactly as it was written; the log's subject joins its first lines. */
export async function getCommitMessage(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<string> {
  const output = await repo.read(["log", "--max-count=1", "--format=%B", sha, "--"], { signal });
  // The log ends each entry with a newline of its own.
  return output.endsWith("\n") ? output.slice(0, -1) : output;
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
  const [branch, push] = await Promise.all([
    currentBranch(repo.read, { signal }),
    // Where the repository's config pushes the branch. It fails when the config can't say, as
    // `git push` would; a broken repository fails the lookups around it too.
    repo.read(["rev-parse", "--symbolic-full-name", "@{push}"], { signal }).then(
      (ref) => ref.trim(),
      () => "",
    ),
  ]);
  // Pushing to a branch of this repository (its remote is `.`) rewrites nothing published.
  if (!branch || (push && !push.startsWith("refs/remotes/"))) return null;
  // When the config can't say, e.g. because the upstream has another name, a guess: the branch of
  // the same name on any remote, which `git push <remote>` updates.
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
