import {
  GitError,
  MergeBlockedError,
  MergeStoppedError,
  RepositoryChangedError,
} from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import {
  currentBranch,
  hasConflicts,
  refExists,
  resolveRef,
  type GitCommand,
  type Repo,
} from "../../core/repo";
import { underWayBlocker } from "../operation/commands";
import { parseRefName } from "../refs/parse";
import { fetchedName } from "../remote/commands";
import { isRef } from "./commands";
import type { MergeOutcome } from "./schema";

/**
 * Merges the branch `ref` (a full ref name, local or remote) into the checked-out branch, `into`,
 * the one the user saw: rejects with `RepositoryChangedError` if another is checked out by now.
 * What it did:
 * - `up-to-date`: the branch has every commit of `ref` already; nothing changed.
 * - `fast-forward`: the branch moved to `ref`, having no commits of its own.
 * - `merged`: a merge commit was made, worded as `git merge` words it ("Merge branch 'feature'").
 * - `conflicts`: the merge stopped at conflicts, left to resolve and commit (see the operations).
 *
 * The user's `merge.ff` setting is kept, as is `merge.log`. It always commits, though, unsquashed,
 * whatever `branch.<name>.mergeOptions` says, and never stashes the uncommitted changes
 * (`merge.autoStash`): changes git won't merge over make it refuse, saying which files to commit or
 * stash; others are kept as they are.
 *
 * Rejects with `MergeBlockedError`, having done nothing, while HEAD is detached, an operation (a
 * merge, a rebase…) or conflicts are under way, or `ref` is gone; with `MergeStoppedError` if a
 * hook turned the merge commit down, which leaves the merge under way, to commit or abort.
 */
export async function mergeBranch(repo: Repo, ref: string, into: string): Promise<MergeOutcome> {
  const target = parseRefName(ref);
  if (!target || target.kind === "tag") throw new MergeBlockedError(`${ref} isn't a branch.`);
  const { gitDir } = await gitDirs(repo);
  // One write, so nothing else changes the branch, or starts another operation, halfway.
  return repo.exclusive(async (run) => {
    // That ref itself, rather than git reading `ref` as a revision, like `refs/heads/main~3`.
    const commit = isRef(run, ref).then((there) => (there ? resolveRef(run, ref) : null));
    const head = resolveRef(run, "HEAD");
    // Read alongside the rest, though only needed once it's checked: it all holds up the other
    // writes. None for a branch without commits yet, which just moves to it: nor can git word one.
    const wording = Promise.all([commit, head]).then(([sha, before]) =>
      sha && before ? mergeMessage(run, sha, ref) : undefined,
    );
    const [blocker, current, sha, before, message, named] = await Promise.all([
      underWayBlocker(gitDir, run, "merge"),
      currentBranch(run),
      commit,
      head,
      wording,
      resolveRef(run, target.name),
    ]);
    if (blocker) throw new MergeBlockedError(blocker);
    if (current === null) {
      throw new MergeBlockedError("HEAD is detached: check out a branch to merge into it.");
    }
    if (current !== into) {
      throw new RepositoryChangedError(
        `Switched from ${into} to ${current} meanwhile, so nothing was merged.`,
      );
    }
    if (!sha) throw new MergeBlockedError(`${target.name} no longer exists.`);
    if (ref === `refs/heads/${into}`) {
      throw new MergeBlockedError(`Can't merge ${into} into itself.`);
    }

    const args = [
      "merge",
      "--quiet",
      "--no-edit",
      // Whatever `branch.<name>.mergeOptions` and `merge.autoStash` say (see above).
      "--commit",
      "--no-squash",
      "--no-autostash",
      // Its log, if `merge.log` wants one, is already in the message.
      "--no-log",
      "--cleanup=strip",
      ...(message === undefined ? [] : ["-m", message]),
      // By its short name, which labels its side of the conflict markers, unless that's another
      // commit's, like a tag's of the same name: then by its full one.
      named === sha ? target.name : ref,
    ];
    let failure: GitError | undefined;
    try {
      // It rewrites the files the branch changed.
      await run(args, { rewritesFiles: true });
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      failure = error;
    }

    if (failure) {
      const [stoppedAtConflicts, merging] = await Promise.all([
        hasConflicts(run),
        refExists(run, "MERGE_HEAD"),
      ]);
      if (stoppedAtConflicts) return "conflicts";
      // Also when the merge commit couldn't be written, e.g. as signing it failed.
      if (merging) {
        throw new MergeStoppedError(
          `Merging ${target.name} stopped before committing:\n${failure.withoutHints().message}\nFix that, then commit the merge, or abort it.`,
        );
      }
      const refused = parseRefusal(failure.stderr);
      if (!refused) throw failure.withoutHints();
      // Git lists the staged files on one line, which can't be told apart if a name has a space.
      const files =
        refused.kind === "staged"
          ? (await run(["diff", "--cached", "--name-only", "-z", "HEAD"]))
              .split("\0")
              .filter(Boolean)
          : refused.files;
      throw failure.withMessage(refusalMessage(refused.kind, target.name, files));
    }
    const after = await resolveRef(run, "HEAD");
    if (after === before) return "up-to-date";
    return (await refExists(run, "HEAD^2")) ? "merged" : "fast-forward";
  });
}

/**
 * The merge commit's message, worded as `git merge` would for the branch `ref`, by its full name:
 * "Merge branch 'feature'", or "Merge remote-tracking branch 'origin/feature'", with " into topic"
 * when merging into a branch other than the main one, and the log `merge.log` asks for.
 */
async function mergeMessage(run: GitCommand, sha: string, ref: string): Promise<string> {
  // A line of FETCH_HEAD, as `git merge` makes for what it merges: "of ." for this repository.
  const message = await run(["fmt-merge-msg"], { stdin: `${sha}\t\t${fetchedName(ref)} of .\n` });
  return message.trim();
}

/**
 * Why git refused to merge over uncommitted changes or untracked files, and the files it named:
 * - `changes`: uncommitted changes to files the merge changes.
 * - `staged`: staged changes, which git won't merge with at all, unless it fast-forwards; their
 *   files aren't listed here (see `parseRefusal`).
 * - `overwritten` and `removed`: untracked files the merge would overwrite or remove.
 * - `folders`: folders the merge replaces, with untracked files in them.
 */
export type RefusalKind = "changes" | "staged" | "overwritten" | "removed" | "folders";

/**
 * Why git refused to merge over uncommitted changes or untracked files, going by what it printed,
 * with the files it listed, each on a line of its own, after a tab; `undefined` if it failed
 * otherwise. Staged changes it lists on one line, two spaces in, separated by spaces, which can't be
 * told apart from spaces in their names: those are left for the caller to ask git for.
 */
export function parseRefusal(stderr: string): { kind: RefusalKind; files: string[] } | undefined {
  if (
    /Your local changes to the following files would be overwritten by merge:\n {2}\S/.test(stderr)
  ) {
    return { kind: "staged", files: [] };
  }
  const patterns: [RegExp, (match: RegExpExecArray) => RefusalKind][] = [
    [/Your local changes to the following files would be overwritten by merge:\n/, () => "changes"],
    [
      /The following untracked working tree files would be (overwritten|removed) by merge:\n/,
      (match) => (match[1] === "removed" ? "removed" : "overwritten"),
    ],
    [/Updating the following directories would lose untracked files in them:\n/, () => "folders"],
  ];
  for (const [pattern, kind] of patterns) {
    const match = pattern.exec(stderr);
    if (!match) continue;
    const files: string[] = [];
    for (const line of stderr.slice(match.index + match[0].length).split("\n")) {
      if (!line.startsWith("\t")) break;
      files.push(line.slice(1));
    }
    if (files.length > 0) return { kind: kind(match), files };
  }
  return undefined;
}

/** What to tell the user when git refused to merge `branch` over `files`, for the reason `kind`. */
export function refusalMessage(kind: RefusalKind, branch: string, files: string[]): string {
  const one = files.length === 1;
  const them = one ? "it" : "them";
  switch (kind) {
    case "changes":
      return `Merging ${branch} would overwrite your uncommitted changes to ${list(files)}. Commit or stash them, then merge.`;
    case "staged":
      return `Git won't merge ${branch} while changes are staged, to ${list(files)}. Commit or stash them, then merge.`;
    case "overwritten":
    case "removed":
      return `Merging ${branch} would ${kind === "removed" ? "remove" : "overwrite"} the untracked ${one ? "file" : "files"} ${list(files)}. Move or remove ${them}, then merge.`;
    case "folders":
      return `Merging ${branch} would lose the untracked files in the ${one ? "folder" : "folders"} ${list(files)}. Move or remove them, then merge.`;
  }
}

/** `files` in a sentence: "a.txt", "a.txt and b.txt", or "a.txt and 3 other files". */
function list(files: string[]): string {
  const [first, second] = files;
  if (files.length === 1) return first!;
  if (files.length === 2) return `${first} and ${second}`;
  return `${first} and ${files.length - 1} other files`;
}
