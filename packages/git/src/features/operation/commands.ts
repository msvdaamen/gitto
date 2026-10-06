import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { GitError, NoOperationError } from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import { currentBranch, hasConflicts, succeeds, type GitCommand, type Repo } from "../../core/repo";
import { rebaseState } from "../remote/progress";
import type { Operation, OperationKind } from "./schema";

/** The contents of `file`, trimmed; `undefined` if it isn't there. */
function readState(file: string): Promise<string | undefined> {
  return readFile(file, "utf8").then(
    (text) => text.trim(),
    () => undefined,
  );
}

/**
 * Which operation is under way in the worktree whose git directory is `gitDir`, going by the
 * files git keeps while it is; `undefined` if none is. A rebase first: one stopped at a merge it
 * replays has a MERGE_HEAD too.
 */
async function operationKind(gitDir: string): Promise<OperationKind | undefined> {
  const [state, merge, cherryPick, revert] = await Promise.all([
    rebaseState(gitDir),
    ...["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"].map((name) =>
      readState(join(gitDir, name)),
    ),
  ]);
  if (state.applying) return "am";
  if (state.rebasing) return "rebase";
  if (merge) return "merge";
  if (cherryPick) return "cherry-pick";
  if (revert) return "revert";
  return undefined;
}

/** The operation under way in the repository, with what the user's told of it; `null` if none. */
export async function getOperation(repo: Repo): Promise<Operation | null> {
  const { gitDir } = await gitDirs(repo);
  const state = (name: string) => readState(join(gitDir, name));
  switch (await operationKind(gitDir)) {
    case "merge": {
      // An octopus merge has a line for each of them; the first is said.
      const [merging = "", into] = await Promise.all([
        state("MERGE_HEAD").then((heads) => heads?.split("\n")[0]),
        currentBranch(repo.read),
      ]);
      return { kind: "merge", merging: await refName(repo.read, merging), into };
    }
    case "rebase": {
      // `rebase-merge` for the merge backend, git's default; `rebase-apply` for the apply one.
      const dir = (await readState(join(gitDir, "rebase-merge", "onto")))
        ? "rebase-merge"
        : "rebase-apply";
      const merging = dir === "rebase-merge";
      const [headName, onto, step, total] = await Promise.all(
        ["head-name", "onto", merging ? "msgnum" : "next", merging ? "end" : "last"].map((name) =>
          state(join(dir, name)),
        ),
      );
      return {
        kind: "rebase",
        branch: headName?.startsWith("refs/heads/") ? headName.slice("refs/heads/".length) : null,
        onto: await refName(repo.read, onto ?? ""),
        steps: steps(step, total),
      };
    }
    case "cherry-pick":
    case "revert": {
      const kind = (await state("CHERRY_PICK_HEAD")) ? "cherry-pick" : "revert";
      const head = await state(kind === "cherry-pick" ? "CHERRY_PICK_HEAD" : "REVERT_HEAD");
      const [commit, todo] = await Promise.all([
        commitName(repo.read, head ?? ""),
        state(join("sequencer", "todo")),
      ]);
      // A line for it and each one after it; the sequencer's comments aside.
      const lines = (todo ?? "").split("\n").filter((line) => /^[^#\s]/.test(line)).length;
      return { kind, commit, remaining: Math.max(lines - 1, 0) };
    }
    case "am": {
      const [step, total] = await Promise.all(
        ["next", "last"].map((name) => state(join("rebase-apply", name))),
      );
      return { kind: "am", steps: steps(step, total) };
    }
    case undefined:
      return null;
  }
}

/** How far an operation has got, from git's numbers for it; `null` if they don't make sense. */
function steps(
  step: string | undefined,
  total: string | undefined,
): { step: number; total: number } | null {
  const at = Number(step);
  const of = Number(total);
  return Number.isInteger(at) && Number.isInteger(of) && at > 0 && of >= at
    ? { step: at, total: of }
    : null;
}

/**
 * The shortest name of the commit `sha`, as the user knows it: a branch pointing at it, or else a
 * remote-tracking branch or a tag, or else its abbreviated SHA.
 */
async function refName(run: GitCommand, sha: string): Promise<string> {
  if (!sha) return "";
  const refs = await run([
    "for-each-ref",
    `--points-at=${sha}`,
    "--format=%(refname)",
    "refs/heads",
    "refs/remotes",
    "refs/tags",
  ]).catch(() => "");
  const names = refs.split("\n").filter(Boolean);
  for (const prefix of ["refs/heads/", "refs/remotes/", "refs/tags/"]) {
    const name = names.find((ref) => ref.startsWith(prefix));
    if (name) return name.slice(prefix.length);
  }
  return (await run(["rev-parse", "--short", sha]).catch(() => sha)).trim();
}

/** The commit `sha`'s abbreviated SHA and subject. */
async function commitName(run: GitCommand, sha: string): Promise<{ sha: string; subject: string }> {
  const output = await run(["log", "-1", "--format=%h%x00%s", sha, "--"]).catch(() => "");
  const [short = sha.slice(0, 7), subject = ""] = output.trim().split("\0");
  return { sha: short, subject };
}

/**
 * The command that continues or aborts `kind`: `git merge --continue`, `git rebase --abort`, and
 * so on.
 */
function operationArgs(kind: OperationKind, action: "continue" | "abort"): string[] {
  return [kind, `--${action}`];
}

/**
 * Checks that the operation under way is `kind`, the one the user saw; rejects with
 * `NoOperationError` if it isn't, e.g. as it was finished in a terminal meanwhile.
 */
async function checkOperation(repo: Repo, kind: OperationKind): Promise<void> {
  const { gitDir } = await gitDirs(repo);
  const current = await operationKind(gitDir);
  if (current !== kind) {
    throw new NoOperationError(
      current
        ? `A ${OPERATION_NAMES[current]} is under way now, not a ${OPERATION_NAMES[kind]}.`
        : `The ${OPERATION_NAMES[kind]} is no longer under way.`,
    );
  }
}

const OPERATION_NAMES: Record<OperationKind, string> = {
  merge: "merge",
  rebase: "rebase",
  "cherry-pick": "cherry-pick",
  revert: "revert",
  am: "git am",
};

/**
 * Continues the operation `kind` once its conflicts are resolved: commits the merge, or the commit
 * cherry-picked, reverted or replayed, with the message git prepared, and goes on with the next.
 * One that stops at the next one's conflicts is left to resolve them too, as git does; that isn't
 * a failure. A cherry-pick or revert whose conflicts were resolved to nothing, which git won't
 * commit, is skipped, as git suggests: there's nothing of it to keep.
 */
export async function continueOperation(repo: Repo, kind: OperationKind): Promise<void> {
  await repo.exclusive(async (run) => {
    const [conflicted] = await Promise.all([hasConflicts(run), checkOperation(repo, kind)]);
    // With the message as git prepared it, as no editor can be opened: `:` takes it as it is, and
    // leaves out its comments, like the conflicts git lists in a merge's.
    const options = { env: { GIT_EDITOR: ":" }, rewritesFiles: true };
    try {
      await goOn(run, operationArgs(kind, "continue"), kind, conflicted);
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      const skips = (kind === "cherry-pick" || kind === "revert") && !conflicted;
      if (!skips || !(await succeeds(run, ["diff", "--cached", "--quiet"]))) {
        throw error.withoutHints();
      }
      await goOn(run, [kind, "--skip"], kind, false).catch((reason: unknown) => {
        throw reason instanceof GitError ? reason.withoutHints() : reason;
      });
    }

    /** Runs `args`, which go on with the operation; resolves if they stop at new conflicts. */
    async function goOn(command: GitCommand, args: string[], of: OperationKind, before: boolean) {
      try {
        await command(args, options);
      } catch (error) {
        // Not if they were there before: git refused to go on with them, and says so.
        if (
          error instanceof GitError &&
          !before &&
          of !== "merge" &&
          (await hasConflicts(command))
        ) {
          return;
        }
        throw error;
      }
    }
  });
}

/**
 * Aborts the operation `kind`, putting the repository back as it was before it started: the
 * conflicts and how they were resolved so far go with it.
 */
export async function abortOperation(repo: Repo, kind: OperationKind): Promise<void> {
  await repo.exclusive(async (run) => {
    await checkOperation(repo, kind);
    try {
      await run(operationArgs(kind, "abort"), { rewritesFiles: true });
    } catch (error) {
      throw error instanceof GitError ? error.withoutHints() : error;
    }
  });
}
