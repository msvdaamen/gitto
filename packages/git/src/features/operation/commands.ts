import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { EditorNeededError, GitError, NoOperationError } from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import {
  currentBranch,
  hasConflicts,
  resolveRef,
  succeeds,
  type GitCommand,
  type Repo,
} from "../../core/repo";
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
 * replays has a MERGE_HEAD too. A series of cherry-picks or reverts is under way while its todo
 * list is there, also once the commit it stopped at was committed by hand, which takes
 * CHERRY_PICK_HEAD away: the rest are still to be picked.
 */
async function operationKind(gitDir: string): Promise<OperationKind | undefined> {
  const [state, merge, cherryPick, revert, todo] = await Promise.all([
    rebaseState(gitDir),
    ...["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", join("sequencer", "todo")].map((name) =>
      readState(join(gitDir, name)),
    ),
  ]);
  if (state.applying) return "am";
  if (state.rebasing) return "rebase";
  if (merge) return "merge";
  if (cherryPick) return "cherry-pick";
  if (revert) return "revert";
  // The todo list's commands say which: `pick` or `revert`.
  const [command] = todoLines(todo)[0]?.split(/\s/) ?? [];
  if (command === "pick" || command === "p") return "cherry-pick";
  if (command === "revert") return "revert";
  return undefined;
}

/** The commands in a todo list (the sequencer's, or a rebase's), its comments and blanks aside. */
function todoLines(todo: string | undefined): string[] {
  return (todo ?? "").split("\n").filter((line) => /^[^#\s]/.test(line));
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
      const [cherryPick, revert, todo] = await Promise.all([
        state("CHERRY_PICK_HEAD"),
        state("REVERT_HEAD"),
        state(join("sequencer", "todo")),
      ]);
      const head = cherryPick ?? revert;
      const kind = cherryPick ? "cherry-pick" : revert ? "revert" : await operationKind(gitDir);
      // None between the commits of a series, once the one it stopped at was committed by hand.
      const commit = head ? await commitName(repo.read, head) : null;
      // A line for the one it stopped at and each one after it.
      const remaining = Math.max(todoLines(todo).length - 1, 0);
      return { kind: kind === "revert" ? "revert" : "cherry-pick", commit, remaining };
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
 * commit, is skipped, as git suggests: there's nothing of it to keep. Only that one: a later one
 * that turns out empty is left for the user, as git leaves it.
 *
 * Rejects with `EditorNeededError` for a rebase that rewords or squashes commits from here on:
 * their messages are the user's to write, in an editor, which Gitto can't open.
 */
export async function continueOperation(repo: Repo, kind: OperationKind): Promise<void> {
  await repo.exclusive(async (run) => {
    const { gitDir } = await gitDirs(repo);
    const [conflicted, before] = await Promise.all([
      hasConflicts(run),
      stoppedAt(run, gitDir),
      checkOperation(repo, kind),
    ]);
    if (kind === "rebase" && (await rewordsOrSquashes(gitDir))) {
      throw new EditorNeededError(
        "The rest of this rebase rewords or squashes commits, whose messages need an editor Gitto can't open. Continue it in a terminal.",
      );
    }
    // With the message as git prepared it, as no editor can be opened: `:` takes it as it is, and
    // leaves out its comments, like the conflicts git lists in a merge's.
    const options = { env: { GIT_EDITOR: ":" }, rewritesFiles: true };
    try {
      await goOn(operationArgs(kind, "continue"), conflicted);
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      // Still at the commit it stopped at, with nothing staged: resolved to nothing.
      const empty =
        (kind === "cherry-pick" || kind === "revert") &&
        !conflicted &&
        before !== undefined &&
        (await stoppedAt(run, gitDir)) === before &&
        (await succeeds(run, ["diff", "--cached", "--quiet"]));
      if (!empty) throw error.withoutHints();
      await goOn([kind, "--skip"], false).catch((reason: unknown) => {
        throw reason instanceof GitError ? reason.withoutHints() : reason;
      });
    }

    /** Runs `args`, which go on with the operation; resolves if they stop at new conflicts. */
    async function goOn(args: string[], conflictedBefore: boolean) {
      try {
        await run(args, options);
      } catch (error) {
        // Not if they were there before: git refused to go on with them, and says so.
        if (
          error instanceof GitError &&
          !conflictedBefore &&
          kind !== "merge" &&
          (await hasConflicts(run))
        ) {
          return;
        }
        throw error;
      }
    }
  });
}

/**
 * Where a cherry-pick or revert stopped: HEAD, and the commit it stopped at; `undefined` if it
 * isn't stopped at one.
 */
async function stoppedAt(run: GitCommand, gitDir: string): Promise<string | undefined> {
  const [head, cherryPick, revert] = await Promise.all([
    resolveRef(run, "HEAD"),
    readState(join(gitDir, "CHERRY_PICK_HEAD")),
    readState(join(gitDir, "REVERT_HEAD")),
  ]);
  const at = cherryPick ?? revert;
  return at && `${head} ${at}`;
}

/**
 * Whether the rebase stopped in the worktree whose git directory is `gitDir` rewords or squashes
 * a commit from here on, the one it stopped at included: git asks for their messages in an editor.
 * `fixup -C` takes the fixup's message as it is; `fixup -c` asks.
 */
async function rewordsOrSquashes(gitDir: string): Promise<boolean> {
  const [todo, done] = await Promise.all([
    readState(join(gitDir, "rebase-merge", "git-rebase-todo")),
    readState(join(gitDir, "rebase-merge", "done")),
  ]);
  const current = todoLines(done).at(-1);
  const left = [...(current ? [current] : []), ...todoLines(todo)];
  return left.some((line) => /^(r|reword|s|squash)\s|^(f|fixup)\s+-c\s/.test(line));
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
