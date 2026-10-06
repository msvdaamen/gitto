import { lstat } from "node:fs/promises";

import {
  DiscardBlockedError,
  GitError,
  LinesNotStageableError,
  PatchChangedError,
} from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import type { GitCommand, Repo } from "../../core/repo";
import type { RunOptions } from "../../core/runner";
import { refuseConflictMarkers } from "../conflicts/commands";
import { getStagedFilePatch, getUnstagedFilePatch } from "../diff/commands";
import { underWayBlocker } from "../operation/commands";
import { linesPatch, type LineAction } from "./lines";
import type { LineSelection, UncommittedSide } from "./schema";

// Paths go to git's stdin, NUL-separated, rather than on the command line, which holds only so
// many (32k characters on Windows).
const PATHS_FROM_STDIN = ["--pathspec-from-file=-", "--pathspec-file-nul"];

/**
 * Stages the files at `paths`, whole. A conflicted one is marked resolved by that, so it isn't
 * while it still has conflict markers: rejects with `ConflictMarkersError`, and stages none.
 */
export async function stage(repo: Repo, paths: string[]): Promise<void> {
  await repo.exclusive(async (run) => {
    await refuseConflictMarkers(repo, run, paths);
    await run(["add", ...PATHS_FROM_STDIN], { stdin: nulSeparated(paths) });
  });
}

/** Unstages the files at `paths`, whole; their changes stay in the working tree. */
export async function unstage(repo: Repo, paths: string[]): Promise<void> {
  // `restore --staged` resets to HEAD, so it can't run before the first commit.
  const args = (await repo.hasHead())
    ? ["restore", "--staged", ...PATHS_FROM_STDIN]
    : ["rm", "--cached", "-r", "-q", ...PATHS_FROM_STDIN];
  await repo.write(args, { stdin: nulSeparated(paths) });
}

/** For a command that puts files in the working tree back, or deletes them (see `rewritesFiles`). */
const REWRITES = { rewritesFiles: true };

function nulSeparated(paths: Iterable<string>): string {
  return Array.from(paths, (path) => `${path}\0`).join("");
}

/** Stages every change, untracked files included; conflicts too, unless markers are left (see `stage`). */
export async function stageAll(repo: Repo): Promise<void> {
  await repo.exclusive(async (run) => {
    await refuseConflictMarkers(repo, run);
    await run(["add", "-A"]);
  });
}

export async function unstageAll(repo: Repo): Promise<void> {
  if (!(await repo.hasHead())) {
    // Nothing to unstage isn't an error.
    await repo.write(["rm", "--cached", "-r", "-q", "--ignore-unmatch", "."]);
    return;
  }
  // Resetting the whole index (`restore --staged .`, `reset`) would also undo conflicts, and plain
  // `reset` even abort the merge. So only the staged changes that aren't conflicts are undone: `u`
  // leaves out unmerged paths, and without renames both sides of one are listed. Each is put back
  // in the index as HEAD has it. Not with `restore --staged` and their paths: git looks every
  // file in the index up in the whole list, which took 6.9s for 40,000 staged files in vscode,
  // against 60ms this way. As one write, so stages still waiting to run are seen, and none comes
  // in between.
  await repo.exclusive(unstageAllButConflicts);
}

/**
 * Puts every staged change but conflicts back in the index as HEAD has it, through `run` (a command
 * of `repo.exclusive`); see `unstageAll`.
 */
async function unstageAllButConflicts(run: GitCommand): Promise<void> {
  // As bytes: a path goes back to git as it came, also one that isn't UTF-8.
  const staged = await run(
    ["diff", "--cached", "--raw", "-z", "--no-abbrev", "--no-renames", "--diff-filter=u"],
    { binary: true },
  );
  // `:<mode in HEAD> <mode> <object in HEAD> <object> <status>`, then the path.
  const fields = staged.split("\0");
  let entries = "";
  for (let i = 0; i + 1 < fields.length; i += 2) {
    const [mode, , object] = fields[i]!.slice(1).split(" ");
    // A file that isn't in HEAD has mode 0 there, which takes it out of the index.
    entries += `${mode} ${object}\t${fields[i + 1]}\0`;
  }
  if (entries) {
    await run(["update-index", "-z", "--index-info"], { stdin: entries, binary: true });
  }
}

/** A file whose lines are staged or unstaged, as its list has it. */
interface LinesFile {
  path: string;
  origPath: string | null;
}

/**
 * Stages the lines `selection` picks from a file's unstaged changes, which the user picked them
 * from in `shown`, their patch (see `getUnstagedFilePatch`). Rejects with `PatchChangedError` if
 * those aren't the file's unstaged changes any more: the lines picked could be others by now.
 * Resolves to the unstaged changes' patch the lines leave, to show in its place.
 */
export function stageLines(
  repo: Repo,
  file: LinesFile & { untracked: boolean },
  shown: string,
  selection: LineSelection,
): Promise<string> {
  return applyLines(repo, shown, selection, "stage", (binary) =>
    // In the index once lines of it are staged, an untracked file is compared to that.
    getUnstagedFilePatch(repo, { ...file, untracked: file.untracked && binary }, { binary }),
  );
}

/**
 * Unstages the lines `selection` picks from a file's staged changes; see `stageLines`. Resolves to
 * the staged changes' patch they leave.
 */
export function unstageLines(
  repo: Repo,
  file: LinesFile,
  shown: string,
  selection: LineSelection,
): Promise<string> {
  return applyLines(repo, shown, selection, "unstage", (binary) =>
    getStagedFilePatch(repo, file, { binary }),
  );
}

/** What `git apply --cached` says of a patch that isn't of the index as it is. */
const INDEX_CHANGED = /patch does not apply|does not exist in index|already exists in index/;

/**
 * Applies the patch that stages or unstages the lines picked from `shown`, built from the file's
 * patch as git has it now, `read`: as one write, so no other one changes the index in between.
 * The patch is read again as bytes, which go back to git as they came, also for a file that isn't
 * UTF-8: one whose lines don't survive being read as it, as the user's were. Resolves to the patch
 * read once they're applied, as text, which saves the UI asking for it.
 */
function applyLines(
  repo: Repo,
  shown: string,
  selection: LineSelection,
  action: LineAction,
  read: (binary: boolean) => Promise<string>,
): Promise<string> {
  const verb = action === "stage" ? "staged" : "unstaged";
  return repo.exclusive(async (run) => {
    const patch = await read(true);
    if (Buffer.from(patch, "latin1").toString("utf8") !== shown) throw new PatchChangedError(verb);
    const lines = linesPatch(patch, selection, action);
    if (lines === undefined) {
      throw new LinesNotStageableError(`None of the selected lines are changes to be ${verb}.`);
    }
    try {
      await run(
        [
          "apply",
          "--cached",
          // Neither warned about nor fixed, whatever `apply.whitespace` says: the lines are staged
          // as they are.
          "--whitespace=nowarn",
          ...(action === "unstage" ? ["--reverse"] : []),
          "-",
        ],
        // Unchanged lines have to match the index's exactly, whatever `apply.ignoreWhitespace`
        // says: they're checked to be the ones the lines were picked among.
        { stdin: lines, binary: true, config: ["apply.ignoreWhitespace=no"] },
      );
    } catch (error) {
      // The index changed since the patch was read, e.g. by a git command run in a terminal.
      if (error instanceof GitError && INDEX_CHANGED.test(error.stderr)) {
        throw new PatchChangedError(verb);
      }
      throw error;
    }
    return read(false);
  });
}

/**
 * The files added with `--intent-to-add`, among `paths` if given: in the index with no contents
 * yet, which putting them back from the index would empty, so they're deleted instead.
 */
async function intendedFiles(
  run: GitCommand,
  paths: string[] = [],
  options?: RunOptions,
): Promise<string[]> {
  // Without renames: one paired with a deleted file would be listed as its rename, not as added.
  const args = ["diff", "--name-only", "-z", "--no-renames", "--diff-filter=A"];
  const output = await run(paths.length ? [...args, "--", ...paths] : args, options);
  return output.split("\0").filter(Boolean);
}

/**
 * Takes the files at `paths` out of the index and deletes them; as bytes if `binary`. Not for many:
 * `rm` looks each file in the index up in all of them.
 */
async function removeFiles(run: GitCommand, paths: string[], binary = false): Promise<void> {
  if (!paths.length) return;
  await run(["rm", "-f", "-q", ...PATHS_FROM_STDIN], {
    stdin: nulSeparated(paths),
    binary,
    ...REWRITES,
  });
}

/**
 * Discards a file's changes on `side`, the side of the uncommitted changes it's listed on; a
 * renamed one's at both its paths (a copy's source isn't the copy's: it's left out). From its
 * unstaged changes, those: a tracked file goes back to how the index has it, and an untracked one,
 * or one added with `--intent-to-add`, is deleted. From its staged changes, all of them: it goes
 * back to how HEAD has it, in the index and the working tree, and one HEAD doesn't have is deleted.
 * Rejects with `DiscardBlockedError`, discarding nothing, for a conflicted file, whose conflicts
 * are resolved instead.
 */
export async function discard(repo: Repo, file: LinesFile, side: UncommittedSide): Promise<void> {
  const paths = file.origPath ? [file.path, file.origPath] : [file.path];
  // On the command line, which holds them: there are two at most. A path also names what's in a
  // folder of that name, which is another file's, so only the paths themselves are kept.
  const own = (listed: string[]) => listed.filter((path) => paths.includes(path));
  await repo.exclusive(async (run) => {
    const [listed, intended] = await Promise.all([
      // The paths in the index, with their stage: a conflict's are 1-3.
      run(["ls-files", "-z", "--format=%(stage) %(path)", "--", ...paths]),
      side === "unstaged" ? intendedFiles(run, paths).then(own) : ([] as string[]),
    ]);
    const entries = listed
      .split("\0")
      .filter(Boolean)
      .map((entry) => ({ stage: entry.slice(0, 1), path: entry.slice(2) }))
      .filter((entry) => paths.includes(entry.path));
    if (entries.some((entry) => entry.stage !== "0")) {
      throw new DiscardBlockedError(
        `${file.path} is conflicted. Resolve its conflicts rather than discarding its changes.`,
      );
    }
    if (side === "staged") {
      // `restore` from HEAD can't run before the first commit, when everything staged is new.
      const args = (await repo.hasHead())
        ? ["restore", "--source=HEAD", "--staged", "--worktree"]
        : ["rm", "-r", "-f", "-q"];
      await run([...args, ...PATHS_FROM_STDIN], { stdin: nulSeparated(paths), ...REWRITES });
      return;
    }
    const tracked = entries.map((entry) => entry.path);
    const restored = tracked.filter((path) => !intended.includes(path));
    const untracked = paths.filter((path) => !tracked.includes(path));
    // `clean` takes no paths from stdin. It leaves ignored files, which aren't listed as untracked,
    // and a repository inside this one, which is: it deletes files, not folders.
    if (untracked.length) await run(["clean", "-f", "-q", "--", ...untracked], REWRITES);
    await removeFiles(run, intended);
    if (restored.length) {
      await run(["restore", "--worktree", ...PATHS_FROM_STDIN], {
        stdin: nulSeparated(restored),
        ...REWRITES,
      });
    }
  });
}

/**
 * Discards every uncommitted change: tracked files go back to how HEAD has them, in the index and
 * the working tree, and the ones HEAD doesn't have are deleted, staged ones too, as are untracked
 * files. Ignored files that aren't staged stay, as do repositories inside this one. A deleted file
 * whose place a folder has taken stays deleted, and the folder as it is: `clean` leaves it. Rejects with `DiscardBlockedError`,
 * discarding nothing, while files are conflicted, which are resolved instead, or while a merge,
 * rebase, cherry-pick, revert or `git am` is under way: its changes are discarded by aborting it.
 * Kept under way, it would go on without them, and a merge be committed with none of the branch's.
 */
export async function discardAll(repo: Repo): Promise<void> {
  const { gitDir } = await gitDirs(repo);
  await repo.exclusive(async (run) => {
    const blocker = await underWayBlocker(gitDir, run, "discard the changes");
    if (blocker) throw new DiscardBlockedError(blocker);
    const hasHead = await repo.hasHead();
    // As bytes: a path goes back to git as it came, also one that isn't UTF-8.
    const bytes = { binary: true };
    const list = async (args: string[]) =>
      (await run([...args, "-z", "--no-renames"], bytes)).split("\0").filter(Boolean);
    if (hasHead) {
      // New files that are ignored, added with `add -f`, which `clean` would leave once they're
      // untracked: deleted while they're staged. Few, as `removeFiles` needs.
      const added = await list(["diff", "--cached", "--name-only", "--diff-filter=A"]);
      await removeFiles(run, await ignoredFiles(run, added), true);
      // The index as `unstageAll` puts it back, rather than `restore --staged` with the paths,
      // which is slow with many.
      await unstageAllButConflicts(run);
      await removeFiles(run, await intendedFiles(run, [], bytes), true);
    } else {
      // Before the first commit, everything staged is new.
      await run(["rm", "-r", "-f", "-q", "--ignore-unmatch", "--", "."], REWRITES);
    }
    // What was staged and HEAD doesn't have is untracked now, and deleted with the rest.
    await run(["clean", "-f", "-d", "-q"], REWRITES);
    if (!hasHead) return;
    // The files that changed in the working tree are written as the index has them. Not a deleted
    // one whose place a folder has taken, which `clean` leaves: writing the file would delete it,
    // and what's in it, like ignored files or a repository.
    const [changed, deleted] = await Promise.all([
      list(["diff", "--name-only"]),
      list(["diff", "--name-only", "--diff-filter=D"]),
    ]);
    const folders = new Set(await foldersAt(repo, deleted));
    const written = changed.filter((path) => !folders.has(path));
    if (written.length) {
      await run(["checkout-index", "-f", "-z", "--stdin"], {
        stdin: nulSeparated(written),
        ...bytes,
        ...REWRITES,
      });
    }
  });
}

/** Those of `paths` (as bytes, see `binary`) that are ignored, whether they're in the index or not. */
async function ignoredFiles(run: GitCommand, paths: string[]): Promise<string[]> {
  if (!paths.length) return [];
  const args = ["check-ignore", "--no-index", "-z", "--stdin"];
  // It takes no pathspec magic, which reading them literally is (see `RunOptions.globPathspecs`).
  const options = { stdin: nulSeparated(paths), binary: true, globPathspecs: true };
  const ignored = await run(args, options).catch(
    // None of them is.
    (error: unknown) => {
      if (error instanceof GitError && error.exitCode === 1) return "";
      throw error;
    },
  );
  return ignored.split("\0").filter(Boolean);
}

/** Those of `paths` (as bytes, see `binary`) where there's a folder in the working tree. */
async function foldersAt(repo: Repo, paths: string[]): Promise<string[]> {
  const root = Buffer.from(`${repo.path}/`);
  const folders = await Promise.all(
    paths.map((path) =>
      lstat(Buffer.concat([root, Buffer.from(path, "latin1")])).then(
        (stats) => stats.isDirectory(),
        () => false,
      ),
    ),
  );
  return paths.filter((_, i) => folders[i]);
}
