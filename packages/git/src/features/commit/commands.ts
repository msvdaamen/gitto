import { GitError, HeadMovedError } from "../../core/errors";
import type { Repo } from "../../core/repo";

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
  await repo.writeTogether(async (run) => {
    if (amend) {
      const head = await run(["rev-parse", "--verify", "--quiet", "HEAD"]).then(
        (sha) => sha.trim(),
        nothingFound,
      );
      if (head !== amend) throw new HeadMovedError();
    }
    // An amended message is kept exactly as sent, whatever `commit.cleanup` says, as what wasn't
    // edited of it is as it was written. It's sent in UTF-8, so it's recorded as that.
    const args = [
      ...["-c", "i18n.commitEncoding=UTF-8", "commit"],
      ...(amend ? ["--amend", "--allow-empty", "--cleanup=verbatim"] : []),
      "-F",
      "-",
    ];
    await run(args, { stdin: message.endsWith("\n") ? message : `${message}\n` });
  });
}

/**
 * For a lookup that exits 1 without a word when there's nothing to find, e.g. `rev-parse --quiet`
 * before the first commit: `""` then, and any other failure rethrown.
 */
function nothingFound(error: unknown): string {
  if (error instanceof GitError && error.exitCode === 1 && !error.stderr.trim()) return "";
  throw error;
}

/** A commit's message exactly as it was written; the log's subject joins its first lines. */
export async function getCommitMessage(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<string> {
  const output = await repo.read(
    // In UTF-8, as it's read, whatever `i18n.logOutputEncoding` says.
    ["log", "--max-count=1", "--encoding=UTF-8", "--format=%B", sha, "--"],
    { signal },
  );
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
  const [head, push] = await Promise.all([
    // In full: `--short` can say `heads/<name>` when a tag has the same name.
    repo
      .read(["symbolic-ref", "--quiet", "HEAD"], { signal })
      .then((ref) => ref.trim(), nothingFound),
    // Where the repository's config pushes the branch. It fails when the config can't say, as
    // `git push` would; a broken repository fails the lookups around it too.
    repo.read(["rev-parse", "--symbolic-full-name", "@{push}"], { signal }).then(
      (ref) => ref.trim(),
      () => "",
    ),
  ]);
  // Pushing to a branch of this repository (its remote is `.`) rewrites nothing published.
  if (!head || (push && !push.startsWith("refs/remotes/"))) return null;
  const branch = head.replace(/^refs\/heads\//, "");
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
