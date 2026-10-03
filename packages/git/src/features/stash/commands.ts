import { GitError, RepositoryChangedError, StashConflictError } from "../../core/errors";
import { resolveRef, type GitCommand, type Repo } from "../../core/repo";
import { parseDiff } from "../diff/parse";
import type { ChangedFile } from "../diff/schema";
import type { Stash } from "./schema";

const FIELDS = ["%H", "%P", "%ct", "%gs"];

/**
 * For the stash commands, which take no paths: with literal pathspecs (see `ENV` in the runner),
 * `stash push --include-untracked` leaves the untracked files it stashed behind, as it removes them
 * with a glob.
 */
const STASH_ENV = { env: { GIT_LITERAL_PATHSPECS: "0" } };

/** The stashes, newest first. */
export async function listStashes(repo: Repo, signal?: AbortSignal): Promise<Stash[]> {
  // With -z, every field and entry ends in a NUL; none of the fields can contain one.
  const output = await repo.read(["stash", "list", "-z", `--format=${FIELDS.join("%x00")}`], {
    signal,
  });
  const fields = output.split("\0");
  const stashes: Stash[] = [];
  for (let i = 0; i + FIELDS.length <= fields.length; i += FIELDS.length) {
    const [sha = "", parents = "", createdAt = "", message = ""] = fields.slice(
      i,
      i + FIELDS.length,
    );
    const base = parents.split(" ")[0]!;
    stashes.push({ sha, base, message, createdAt: Number(createdAt) * 1000 });
  }
  return stashes;
}

/**
 * Stashes every uncommitted change, staged or not, untracked files included, so none are left.
 * Git refuses before the first commit, and with conflicts, saying so. With nothing to stash, it
 * does nothing.
 */
export async function pushStash(repo: Repo): Promise<void> {
  // Not `--quiet`, which keeps git from saying why it refused, too.
  await repo.write(["stash", "push", "--include-untracked"], STASH_ENV);
}

/** The files the stash `sha` changed compared to its base, untracked ones included. */
export async function getStashFiles(
  repo: Repo,
  sha: string,
  signal?: AbortSignal,
): Promise<ChangedFile[]> {
  // Diff options of its own, so `stash.showStat` and `stash.showPatch` don't add any.
  const output = await repo.read(
    ["stash", "show", "--include-untracked", "-M", "--raw", "--numstat", "-z", sha],
    { ...STASH_ENV, signal },
  );
  return parseDiff(output);
}

/**
 * Puts the stash `sha` back and drops it, as long as it's still the newest. What was staged when it
 * was stashed is staged again, if that can be done safely (see below). A pop that conflicts is left
 * to resolve, and the stash kept, as git does.
 */
export async function popStash(repo: Repo, sha: string): Promise<void> {
  // One write, so no other stash of Gitto's can become the newest between checking and popping.
  await repo.exclusive(async (run) => {
    if ((await resolveRef(run, "refs/stash")) !== sha) {
      throw new RepositoryChangedError(
        "The stashes changed before the stash could be popped, so nothing was popped.",
      );
    }
    const [conflicted, staged] = await Promise.all([hasConflicts(run), hasStagedChanges(run)]);
    try {
      // Putting back what was staged (`--index`) is only tried when nothing is staged now: when
      // something is, a pop that fails partway can unstage it, and leave half the stash applied.
      // Without staged changes, it either works, fails like a plain pop, or fails before changing
      // anything when the staged changes don't apply, and a plain pop follows.
      if (!staged && (await popIndex(run))) return;
      await run(["stash", "pop", "--quiet"], STASH_ENV);
    } catch (error) {
      // Git won't pop over conflicts that were already there, and says so.
      if (error instanceof GitError && !conflicted && (await hasConflicts(run))) {
        throw new StashConflictError(
          "Popping the stash caused conflicts. Resolve them. The stash was kept, as git does when a pop conflicts.",
        );
      }
      throw error;
    }
  });
}

/**
 * Pops the newest stash with what was staged staged again; `false`, having changed nothing, when
 * that won't apply to the index.
 */
function popIndex(run: GitCommand): Promise<boolean> {
  return run(["stash", "pop", "--index", "--quiet"], STASH_ENV).then(
    () => true,
    (error: unknown) => {
      if (!(error instanceof GitError)) throw error;
      if (/conflicts in index/i.test(error.stderr)) return false;
      // Said when the pop was refused, e.g. as it'd overwrite local changes: a plain pop wouldn't
      // have restaged anything either, so that's not news.
      const message = error.message.replace(/^Index was not unstashed\.\n?/m, "").trim();
      if (!message || message === error.message) throw error;
      const Class = error.constructor as typeof GitError;
      throw new Class(message, error.args, error.exitCode, error.stderr);
    },
  );
}

async function hasConflicts(run: GitCommand): Promise<boolean> {
  return (await run(["ls-files", "--unmerged"])) !== "";
}

/** Whether anything is staged; rejects if git couldn't tell. */
function hasStagedChanges(run: GitCommand): Promise<boolean> {
  // Exits with 1 when something is.
  return run(["diff", "--cached", "--quiet"]).then(
    () => false,
    (error: unknown) => {
      if (error instanceof GitError && error.exitCode === 1) return true;
      throw error;
    },
  );
}
