import {
  GitError,
  RebaseBlockedError,
  RebaseStoppedError,
  RebaseUndoneError,
  RepositoryChangedError,
} from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import {
  currentBranch,
  hasConflicts,
  resolveRef,
  type GitCommand,
  type Repo,
} from "../../core/repo";
import { OPERATION_NAMES, operationUnderWay } from "../operation/commands";
import { parseRefName } from "../refs/parse";
import { rebaseState } from "../remote/progress";
import { isRef } from "./commands";
import { list, parseRefusal, refusalMessage, stagedFiles } from "./merge";
import type { RebaseOutcome } from "./schema";

/**
 * Rebases the checked-out branch, `branch`, the one the user saw, onto the branch `ref` (a full
 * ref name, local or remote): replays the commits only it has on top of `ref`'s, one by one.
 * Rejects with `RepositoryChangedError` if another branch is checked out by now. What it did:
 * - `up-to-date`: the branch was on top of `ref` already; nothing changed.
 * - `fast-forward`: the branch moved to `ref`, with no commits of its own to replay, or none that
 *   `ref` didn't have already, which git leaves out.
 * - `rebased`: its commits were replayed on top of `ref`.
 * - `conflicts`: the rebase stopped at a commit's conflicts, left to resolve and continue (see the
 *   operations); it may stop again at the next commit's.
 *
 * The user's `rebase.*` settings are kept, like `rebase.rebaseMerges` and `rebase.updateRefs`,
 * except these: it never stashes the uncommitted changes (`rebase.autoStash`), which git would
 * otherwise put back after rebasing, or leave in the stash without a word if they conflict, but
 * refuses while there are any, saying which files to commit or stash; and it doesn't squash
 * `fixup!` and `squash!` commits (`rebase.autoSquash`), whose messages would need an editor Gitto
 * can't open. Untracked files are kept as they are, unless one is in the way of `ref`'s files,
 * which makes it refuse too.
 *
 * Rejects with `RebaseBlockedError`, having done nothing, while HEAD is detached or on a branch
 * without commits yet, an operation (a merge, a rebase…) or conflicts are under way, `ref` is
 * gone, or there are uncommitted changes or untracked files in the way. One that stops partway
 * without conflicts, as a commit couldn't be signed say, is undone, and rejects with
 * `RebaseUndoneError` saying why: git leaves such a commit's changes staged, for the user to
 * commit by hand before it goes on, which Gitto can't do for them. With `RebaseStoppedError` if it
 * couldn't be undone either, which leaves it under way, to finish in a terminal or abort.
 */
export async function rebaseBranch(
  repo: Repo,
  ref: string,
  branch: string,
): Promise<RebaseOutcome> {
  const target = parseRefName(ref);
  if (!target || target.kind === "tag") throw new RebaseBlockedError(`${ref} isn't a branch.`);
  const { gitDir } = await gitDirs(repo);
  // One write, so nothing else changes the branch, or starts another operation, halfway.
  return repo.exclusive(async (run) => {
    // That ref itself, rather than git reading `ref` as a revision, like `refs/heads/main~3`.
    const onto = isRef(run, ref).then((there) => (there ? resolveRef(run, ref) : null));
    const [operation, conflicted, current, sha, before] = await Promise.all([
      operationUnderWay(gitDir),
      hasConflicts(run),
      currentBranch(run),
      onto,
      resolveRef(run, "HEAD"),
    ]);
    if (operation) {
      throw new RebaseBlockedError(
        `A ${OPERATION_NAMES[operation]} is under way. Finish or abort it, then rebase.`,
      );
    }
    if (conflicted) {
      throw new RebaseBlockedError("Some files have conflicts. Resolve them, then rebase.");
    }
    if (current === null) {
      throw new RebaseBlockedError("HEAD is detached: check out a branch to rebase it.");
    }
    if (current !== branch) {
      throw new RepositoryChangedError(
        `Switched from ${branch} to ${current} meanwhile, so nothing was rebased.`,
      );
    }
    if (!sha) throw new RebaseBlockedError(`${target.name} no longer exists.`);
    if (ref === `refs/heads/${branch}`) {
      throw new RebaseBlockedError(`Can't rebase ${branch} onto itself.`);
    }
    if (!before) {
      throw new RebaseBlockedError(
        `${branch} has no commits yet, so there's nothing to rebase. Merge ${target.name} into it instead.`,
      );
    }

    const args = [
      "rebase",
      "--quiet",
      // Whatever `rebase.autoStash` and `rebase.autoSquash` say (see above).
      "--no-autostash",
      "--no-autosquash",
      // By its full name, which can't be mistaken for another ref's, like a tag's of the same name.
      ref,
    ];
    let failure: GitError | undefined;
    try {
      // With any message as git prepared it, as no editor can be opened. It rewrites the files
      // the branches changed.
      await run(args, { env: { GIT_EDITOR: ":" }, rewritesFiles: true });
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      failure = error;
    }

    if (failure) {
      const [stoppedAtConflicts, { rebasing }] = await Promise.all([
        hasConflicts(run),
        rebaseState(gitDir),
      ]);
      if (stoppedAtConflicts) return "conflicts";
      // Stopped without them: a commit couldn't be written, e.g. as signing it failed. Undone,
      // rather than left to continue (see above).
      if (rebasing) {
        const reason = failure.withoutHints().message;
        const undone = await run(["rebase", "--abort"], { rewritesFiles: true }).then(
          () => true,
          () => false,
        );
        if (!undone) {
          throw new RebaseStoppedError(
            `Rebasing ${branch} onto ${target.name} stopped partway:\n${reason}\nFinish the rebase in a terminal, or abort it.`,
          );
        }
        throw new RebaseUndoneError(
          `Rebasing ${branch} onto ${target.name} stopped partway, and was undone:\n${reason}\nFix that, then rebase again.`,
        );
      }
      const refused = await whyRefused(run, failure, target.name);
      if (refused) throw new RebaseBlockedError(refused);
      throw failure.withoutHints();
    }
    const after = await resolveRef(run, "HEAD");
    if (after === before) return "up-to-date";
    return after === sha ? "fast-forward" : "rebased";
  });
}

/**
 * What to tell the user about `failure`, if git refused to rebase onto `onto` with it, having
 * done nothing: over uncommitted changes, which it won't rebase with at all, naming the files, or
 * untracked files in the way of checking out `onto`; `undefined` if it failed otherwise.
 */
async function whyRefused(
  run: GitCommand,
  failure: GitError,
  onto: string,
): Promise<string | undefined> {
  const changes = parseUncommitted(failure.stderr);
  if (changes) {
    // Git doesn't name them: it checks the working tree first, then the index.
    const files =
      changes === "unstaged"
        ? (await run(["diff", "--name-only", "-z"])).split("\0").filter(Boolean)
        : await stagedFiles(run);
    const named = files.length > 0;
    return changes === "unstaged"
      ? `Git won't rebase while there are uncommitted changes${named ? ` to ${list(files)}` : ""}. Commit or stash them, then rebase.`
      : `Git won't rebase while changes are staged${named ? `, to ${list(files)}` : ""}. Commit or stash them, then rebase.`;
  }
  const refused = parseRefusal(failure.stderr);
  if (!refused) return undefined;
  // Git lists staged changes without naming their files apart (see `parseRefusal`).
  const files = refused.kind === "staged" ? await stagedFiles(run) : refused.files;
  return refusalMessage(refused.kind, files, {
    doing: `Rebasing onto ${onto}`,
    wont: "rebase",
    next: "rebase",
  });
}

/**
 * Whether git refused to rebase over uncommitted changes, going by what it printed, and which:
 * unstaged ones ("You have unstaged changes") or staged ones ("Your index contains uncommitted
 * changes"); `undefined` if it failed otherwise.
 */
export function parseUncommitted(stderr: string): "unstaged" | "staged" | undefined {
  if (/cannot rebase: You have unstaged changes\./.test(stderr)) return "unstaged";
  if (/cannot rebase: Your index contains uncommitted changes\./.test(stderr)) return "staged";
  return undefined;
}
