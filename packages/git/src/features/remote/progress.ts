import { stat } from "node:fs/promises";
import { join } from "node:path";

import { PullInterruptedError, type GitError } from "../../core/errors";
import { hasConflicts, refExists, succeeds, type GitCommand } from "../../core/repo";

// What a merge, rebase or `git am` left unfinished in a repository, and what to tell the user
// about a pull that stopped at it.

/** What a merge, rebase or `git am` left unfinished in a repository. */
export interface Progress {
  conflicts: boolean;
  merging: boolean;
  rebasing: boolean;
  /** Applying patches with `git am`, which keeps its state where some rebases do. */
  applying: boolean;
}

/** Whether `state` has anything unfinished, which git won't merge or rebase over. */
export function inProgress(state: Progress): boolean {
  return state.conflicts || state.merging || state.rebasing || state.applying;
}

/** Whether `file` is there; a folder counts. */
function fileExists(file: string): Promise<boolean> {
  return stat(file).then(
    () => true,
    () => false,
  );
}

/**
 * Whether a rebase, or `git am`, is stopped in the worktree whose git directory is `gitDir`: they
 * keep their state there meanwhile, `am` where some rebases do, with a file saying it's `am`'s.
 */
export async function rebaseState(
  gitDir: string,
): Promise<{ rebasing: boolean; applying: boolean }> {
  const [rebaseMerge, rebaseApply, applying] = await Promise.all(
    ["rebase-merge", "rebase-apply", "rebase-apply/applying"].map((name) =>
      fileExists(join(gitDir, name)),
    ),
  );
  return { rebasing: !!rebaseMerge || (!!rebaseApply && !applying), applying: !!applying };
}

/** What's unfinished in the repository, read at once. */
export async function progress(run: GitCommand, gitDir: string): Promise<Progress> {
  const [conflicts, merging, state] = await Promise.all([
    hasConflicts(run),
    refExists(run, "MERGE_HEAD"),
    rebaseState(gitDir),
  ]);
  return { conflicts, merging, ...state };
}

/** Whether `ancestor` is in `rev`'s history; rejects if git couldn't tell. */
function isAncestor(run: GitCommand, ancestor: string, rev: string): Promise<boolean> {
  return succeeds(run, ["merge-base", "--is-ancestor", ancestor, rev]);
}

/**
 * Why the merge or rebase `args`, which `failure` is of if it failed, left the pull for the user
 * to finish, now that it's done; `undefined` if it didn't. Even one that went through can have:
 * putting back local changes it stashed can conflict, and `branch.<name>.mergeOptions` can stop a
 * merge before committing, or squash it, so what was merged isn't in HEAD.
 */
export async function interruption(
  run: GitCommand,
  gitDir: string,
  args: string[],
  upstream: string,
  failure: GitError | undefined,
): Promise<PullInterruptedError | undefined> {
  const after = await progress(run, gitDir);
  // A merge that went through, or stopped at conflicts, without a merge to commit: squashed, if
  // what it merged isn't in HEAD.
  const squashing =
    args[0] === "merge" &&
    !after.merging &&
    (!failure || after.conflicts) &&
    !(await isAncestor(run, args.at(-1)!, "HEAD"));
  if (!after.conflicts && !after.merging && !after.rebasing && !squashing) return undefined;
  const name = upstream.replace(/^refs\/(remotes|heads)\//, "");
  return interruptedError(name, { ...after, squashing }, failure);
}

/** Says where the pull stopped, and what to do to finish it. */
function interruptedError(
  upstream: string,
  { conflicts, merging, rebasing, squashing }: Progress & { squashing: boolean },
  failure: GitError | undefined,
): PullInterruptedError {
  // Why git stopped, when it wasn't at conflicts: e.g. an untracked file in the way of a commit
  // a rebase replays, or a hook that turned a merge commit down.
  const reason = failure?.withoutHints().message;
  const stopped = (what: string, next: string) =>
    reason ? `${what}:\n${reason}\nFix that, then ${next}.` : `${what}. ${capitalize(next)}.`;
  let message: string;
  if (rebasing && !conflicts) {
    message = stopped(
      `Rebasing onto ${upstream} stopped partway`,
      "continue the rebase, or abort it",
    );
  } else if (rebasing) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then continue the rebase.`;
  } else if (merging && !conflicts) {
    message = stopped(
      `Merging ${upstream} stopped before committing`,
      "commit the merge, or abort it",
    );
  } else if (merging) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then commit the merge.`;
  } else if (squashing && !conflicts) {
    message = `Squashed ${upstream} into the staged changes, without committing. Commit them to finish pulling.`;
  } else if (squashing) {
    message = `Pulling ${upstream} caused conflicts. Resolve them, then commit the squashed changes.`;
  } else {
    // The pull stashed local changes (rebase.autoStash, merge.autoStash) and they conflict.
    message = `Pulled ${upstream}, but your local changes conflict with it. Resolve the conflicts; your changes are also kept in the stash.`;
  }
  return new PullInterruptedError(message);
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
