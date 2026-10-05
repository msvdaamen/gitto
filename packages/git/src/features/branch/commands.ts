import { lstat } from "node:fs/promises";
import { join } from "node:path";

import { ChangesStashedError, GitError } from "../../core/errors";
import { currentBranch, resolveRef, type GitCommand, type Repo } from "../../core/repo";
import type { RunOptions } from "../../core/runner";
import { parseRefName } from "../refs/parse";
import type { RefKind } from "../refs/schema";
import { popNewest, stashChanges } from "../stash/commands";

/**
 * Creates the branch `name` and switches to it: at HEAD, or at the branch `from` (a full ref name),
 * which must be there.
 * At HEAD, it stays on the same commit, so every uncommitted change, staged or not, comes along to
 * the new branch as it is; from a branch, they come along as when switching to it (see
 * `switchBranch`), through the stash if need be. It doesn't track the branch it was made from, so
 * pulling it doesn't pull that one's commits into it. Git refuses a name that's taken or not a valid
 * branch name, and to switch in the middle of a merge or rebase, saying so.
 */
export async function createBranch(repo: Repo, name: string, from?: string): Promise<void> {
  const args = ["switch", "--no-track", "--create", name];
  // Without the hint pointing to `git help check-ref-format` after an invalid name.
  const options = { config: ["advice.refSyntax=false"] };
  if (from === undefined) {
    await repo.write(args, options);
    return;
  }
  const target = parseRefName(from);
  if (!target || target.kind === "tag") throw new Error(`${from} isn't a branch.`);
  // One write, so nothing else changes the branches, the stashes or the working tree halfway.
  await repo.exclusive(async (run) => {
    // That ref itself, rather than git reading `from` as a revision, like `refs/heads/main~3`.
    if (!(await isRef(run, from))) throw new Error(`${from} isn't a branch.`);
    await switchTakingChanges(run, repo.path, [...args, "--", from], name, options);
  });
}

/** Whether `ref` is the full name of a ref that's there. */
async function isRef(run: GitCommand, ref: string): Promise<boolean> {
  return run(["show-ref", "--verify", "--quiet", ref]).then(
    () => true,
    (error: unknown) => {
      // Exits with 1, saying nothing, when there's no such ref.
      if (error instanceof GitError && error.exitCode === 1) return false;
      throw error;
    },
  );
}

/**
 * What git says when it won't switch as it'd lose uncommitted changes or untracked files: ones the
 * branch would overwrite or remove, or that are in a folder it replaces with a file.
 */
const WOULD_LOSE = /would be (overwritten|removed) by checkout|would lose untracked files/;

/**
 * Switches to the branch `ref` (a full ref name), taking the uncommitted changes along, staged or
 * not, untracked files included. For a remote branch, that's the local branch tracking it (the
 * current one, or the one named after it, if several do), or a new one that does, named after it
 * without the remote (e.g. `feature` for `origin/feature`); git refuses if a local branch that
 * doesn't track it has that name.
 *
 * When git won't switch with the changes, as the branch changes the files they're in, they're
 * stashed, the branch switched to, and the stash popped there; if it wouldn't pop without
 * conflicts, it's kept, and that's thrown as a `ChangesStashedError`. Git refuses to switch in the
 * middle of a merge or rebase, saying so.
 */
export async function switchBranch(repo: Repo, ref: string): Promise<void> {
  const target = parseRefName(ref);
  if (!target || target.kind === "tag") throw new Error(`${ref} isn't a branch.`);
  // One write, so nothing else changes the branches, the stashes or the working tree halfway.
  await repo.exclusive(async (run) =>
    switchTakingChanges(run, repo.path, await switchArgs(run, ref, target), target.name),
  );
}

/**
 * Runs the `git switch` `args`, to the branch `name`, taking the uncommitted changes along: when
 * git won't, as the branch changes the files they're in, they're stashed, it's switched, and the
 * stash popped there; if it wouldn't pop without conflicts, it's kept, and that's thrown as a
 * `ChangesStashedError`. For a repository at `root`, in a write of its own (`Repo.exclusive`).
 */
async function switchTakingChanges(
  run: GitCommand,
  root: string,
  args: string[],
  name: string,
  options?: RunOptions,
): Promise<void> {
  // It rewrites the files that differ between the branches.
  const switching = { ...options, rewritesFiles: true };
  let refused: GitError;
  try {
    // Git takes the changes along itself, as long as the branch doesn't change their files.
    await run(args, switching);
    return;
  } catch (error) {
    if (!(error instanceof GitError && WOULD_LOSE.test(error.stderr))) throw error;
    refused = error;
  }

  const stash = await stashToSwitch(run, name, refused);
  const from = await headPosition(run);
  let failed: unknown;
  try {
    await run(args, switching);
  } catch (error) {
    // Back as they were, on the branch they were made on, when it didn't switch.
    if ((await headPosition(run)) === from) {
      await popNewest(run);
      throw error;
    }
    // It switched, but failed after, at a post-checkout hook, say: thrown once they're back.
    failed = error;
  }
  if (!(await popsCleanly(run, root, stash))) {
    throw new ChangesStashedError(
      `Switched to ${(await currentBranch(run)) ?? "the branch"}, but your uncommitted changes conflict with it, so they're kept in the stash.`,
    );
  }
  await popNewest(run);
  if (failed) throw failed;
}

/** The `git switch` that switches to the branch `ref`, of `target`'s kind and short name. */
async function switchArgs(
  run: GitCommand,
  ref: string,
  target: { kind: RefKind; name: string },
): Promise<string[]> {
  const { name } = target;
  // `--no-guess`: never a new branch made from a remote one of the same name.
  if (target.kind !== "remote") return ["switch", "--no-guess", "--", name];

  // With NUL-separated fields: ref names can't contain one.
  const [locals, remotes] = await Promise.all([
    run(["for-each-ref", "--format=%(refname:lstrip=2)%00%(upstream)%00%(HEAD)", "refs/heads/"]),
    run(["remote"]),
  ]);
  // The remote is the longest one the name starts with, as a remote's name can have a slash in it;
  // else (the remote's gone, its branches left) the name's first part.
  const remote = remotes
    .split("\n")
    .filter((candidate) => candidate && name.startsWith(`${candidate}/`))
    .reduce((longest, candidate) => (candidate.length > longest.length ? candidate : longest), "");
  const local = name.slice((remote ? remote.length : name.indexOf("/")) + 1);

  const tracking = locals
    .split("\n")
    .map((line) => line.split("\0"))
    .filter(([, upstream]) => upstream === ref);
  const branch =
    tracking.find(([, , head]) => head === "*") ??
    tracking.find(([candidate]) => candidate === local) ??
    tracking[0];
  if (branch) return ["switch", "--no-guess", "--", branch[0]!];
  return ["switch", `--create=${local}`, "--track", "--", ref];
}

/**
 * Stashes the uncommitted changes git `refused` to switch to `name` with, and returns the stash.
 * When nothing could be stashed (before the first commit, say), `refused` is thrown: why git
 * wouldn't switch says more than why it wouldn't stash.
 */
async function stashToSwitch(run: GitCommand, name: string, refused: GitError): Promise<string> {
  const before = await resolveRef(run, "refs/stash");
  let failure: unknown;
  await stashChanges(run, `Uncommitted changes when switching to ${name}`).catch(
    (error: unknown) => (failure = error),
  );
  const stash = await resolveRef(run, "refs/stash");
  if (!stash || stash === before) throw refused;
  // Git can fail after making the stash, e.g. removing an untracked file it stashed.
  if (failure) {
    const reason = failure instanceof Error ? ` ${failure.message}` : "";
    throw new ChangesStashedError(
      `Stashing your uncommitted changes failed partway, so they're in the stash, and some may still be here too.${reason}`,
    );
  }
  return stash;
}

/** The branch HEAD is on, or the commit, when it's detached. */
async function headPosition(run: GitCommand): Promise<string | null> {
  return (await currentBranch(run)) ?? (await resolveRef(run, "HEAD"));
}

/**
 * Whether the stash `sha` would pop without conflicts, onto a working tree without changes: its
 * changes merge with HEAD's, and none of its untracked files is in the way of a file that's there.
 * Checked up front, as a pop that fails can leave some of the changes applied.
 */
async function popsCleanly(run: GitCommand, root: string, sha: string): Promise<boolean> {
  // The merge a pop makes, without touching the working tree; exits with 1 at conflicts.
  const merges = await run([
    "merge-tree",
    "--write-tree",
    `--merge-base=${sha}^1`,
    "HEAD",
    sha,
  ]).then(
    () => true,
    (error: unknown) => {
      if (error instanceof GitError && error.exitCode === 1) return false;
      throw error;
    },
  );
  if (!merges) return false;

  // The untracked files are in a third parent, if it stashed any.
  if (!(await resolveRef(run, `${sha}^3`))) return true;
  const untracked = (await run(["ls-tree", "-r", "-z", "--name-only", `${sha}^3`]))
    .split("\0")
    .filter(Boolean);
  // In batches, stopping at the first that's in the way: there can be thousands.
  for (let i = 0; i < untracked.length; i += 100) {
    const batch = untracked.slice(i, i + 100);
    // oxlint-disable-next-line no-await-in-loop -- one batch at a time, on purpose.
    if ((await Promise.all(batch.map((file) => exists(join(root, file))))).includes(true)) {
      return false;
    }
  }
  return true;
}

/** Whether something is at `path`, or a file is where one of its folders would go. */
function exists(path: string): Promise<boolean> {
  return lstat(path).then(
    () => true,
    // ENOTDIR when one of its folders is a file; anything else may be in the way too.
    (error: NodeJS.ErrnoException) => error.code !== "ENOENT",
  );
}
