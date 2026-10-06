import { entriesAt, entryAt } from "../../core/entry";
import {
  DiscardBlockedError,
  GitError,
  LinesNotStageableError,
  PatchChangedError,
} from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import type { GitCommand, Repo } from "../../core/repo";
import { refuseConflictMarkers } from "../conflicts/commands";
import { getStagedFilePatch, getUnstagedFilePatch } from "../diff/commands";
import { operationBlocker, underWayBlocker } from "../operation/commands";
import { parseStatus, STATUS_ARGS } from "../status/parse";
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

function nulSeparated(paths: string[]): string {
  return paths.map((path) => `${path}\0`).join("");
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
  await repo.exclusive(async (run) => {
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
  });
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
 * `paths` as pathspecs that name them alone, not what's in a folder of the same name, which is
 * another file's; for a command run with `globPathspecs`, which reads their magic.
 */
function exactly(paths: string[]): string[] {
  return paths.flatMap((path) => [`:(literal)${path}`, `:(exclude,literal)${path}/`]);
}

/** For a command given `exactly` those `paths`, from its stdin. */
function exactlyFromStdin(paths: string[]) {
  return { stdin: nulSeparated(exactly(paths)), globPathspecs: true, ...REWRITES };
}

/**
 * Discards a file's changes on `side`, the side of the uncommitted changes it's listed on; a
 * renamed one's at both its paths. A copy's source is another file, and left as it is. From its
 * unstaged changes, those: a tracked file goes back to how the index has it, and an untracked one,
 * or one added with `--intent-to-add`, is deleted. From its staged changes, all of them: it goes
 * back to how HEAD has it, in the index and the working tree, and one HEAD doesn't have is deleted.
 *
 * Rejects with `DiscardBlockedError`, discarding nothing: for a conflicted file, whose conflicts are
 * resolved instead; for a submodule, whose changes are discarded in it; for a file's staged changes
 * while an operation like a merge is under way, which would go on without them; and where putting
 * a file back would write over something that isn't it: a folder that has taken its place, or a
 * new file at a rename's previous path.
 */
export async function discard(repo: Repo, file: LinesFile, side: UncommittedSide): Promise<void> {
  const { path, origPath } = file;
  const paths = origPath ? [path, origPath] : [path];
  const named = new Set(paths);
  const gitDir = side === "staged" ? (await gitDirs(repo)).gitDir : undefined;
  await repo.exclusive(async (run) => {
    // On the command line, which holds them: there are two at most.
    const listed = async (args: string[]) =>
      (await run([...args, "-z", "--", ...paths])).split("\0").filter(Boolean);
    const [entries, added, hasHead, blocker, there] = await Promise.all([
      // The paths in the index, with their mode and stage: a conflict's are 1-3.
      listed(["ls-files", "--format=%(objectmode) %(stage) %(path)"]),
      // Added with `--intent-to-add`: in the index with no contents yet, which `restore` would put
      // back, emptying the file, so it's deleted instead. Without renames: one paired with a
      // deleted file would be listed as its rename, not as added.
      side === "unstaged"
        ? listed(["diff", "--name-only", "--no-renames", "--diff-filter=A"])
        : ([] as string[]),
      side === "staged" && repo.hasHead(),
      gitDir && operationBlocker(gitDir, "discard its staged changes"),
      Promise.all(paths.map((each) => entryAt(repo.path, each))),
    ]);
    if (blocker) throw new DiscardBlockedError(blocker);
    // Only the paths themselves, not what's in a folder of the same name, which is another file's.
    const tracked = new Set<string>();
    for (const entry of entries) {
      // `<mode> <stage> <path>`, the mode six digits.
      const entryPath = entry.slice(9);
      if (!named.has(entryPath)) continue;
      if (entry[7] !== "0") {
        throw new DiscardBlockedError(
          `${path} is conflicted. Resolve its conflicts rather than discarding its changes.`,
        );
      }
      if (entry.startsWith("160000")) {
        throw new DiscardBlockedError(
          `${entryPath} is a submodule, a repository of its own: discard its changes in it.`,
        );
      }
      tracked.add(entryPath);
    }
    const intended = new Set(added.filter((each) => named.has(each)));
    const at = (each: string) => there[paths.indexOf(each)];
    // A copy's source is still there on this side, unlike a rename's: in the index, or in the
    // working tree.
    const copied =
      origPath !== null && (side === "staged" ? tracked.has(origPath) : at(origPath) !== undefined);
    const discarded = copied ? [path] : paths;
    // Put back as HEAD or the index has them, over what's in the working tree.
    const written =
      side === "staged"
        ? discarded
        : discarded.filter((each) => tracked.has(each) && !intended.has(each));
    for (const each of written) {
      const entry = at(each);
      // The file itself is fine to write over, tracked or not, but not a folder, nor a new file at
      // a rename's previous path.
      if (entry === "folder" || entry === "blocked" || (entry && each !== path)) {
        throw new DiscardBlockedError(
          `${entry === "folder" ? "A folder" : "Something else"} is at ${each} now. Move or delete it, then discard the changes.`,
        );
      }
    }
    if (side === "staged") {
      // `restore` from HEAD can't run before the first commit, when everything staged is new.
      const args = hasHead
        ? ["restore", "--source=HEAD", "--staged", "--worktree"]
        : ["rm", "-r", "-f", "-q"];
      await run([...args, ...PATHS_FROM_STDIN], exactlyFromStdin(discarded));
      return;
    }
    const untracked = discarded.filter((each) => !tracked.has(each));
    // `clean` takes no paths from stdin. It leaves ignored files, which aren't listed as untracked,
    // and a repository inside this one, which is: it deletes files, not folders.
    if (untracked.length) {
      await run(["clean", "-f", "-q", "--", ...exactly(untracked)], {
        globPathspecs: true,
        ...REWRITES,
      });
    }
    if (intended.size)
      await run(["rm", "-f", "-q", ...PATHS_FROM_STDIN], exactlyFromStdin([...intended]));
    if (written.length) {
      await run(["restore", "--worktree", ...PATHS_FROM_STDIN], exactlyFromStdin(written));
    }
  });
}

/**
 * Discards every uncommitted change: tracked files go back to how HEAD has them, in the index and
 * the working tree, and the ones HEAD doesn't have are deleted, staged ones too, ignored or not, as
 * are untracked files. Ignored files that aren't staged stay. Resolves to the paths of the changes
 * it keeps, which it can't discard: a submodule's and a repository's inside this one, which are
 * discarded in them, and a deleted file's where putting it back would delete what's taken its
 * place: a folder, or a file where one of its folders was.
 *
 * Rejects with `DiscardBlockedError`, discarding nothing, while files are conflicted, which are
 * resolved instead, or while a merge, rebase, cherry-pick, revert or `git am` is under way: its
 * changes are discarded by aborting it. Kept under way, it would go on without them, and a merge be
 * committed with none of the branch's.
 */
export async function discardAll(repo: Repo): Promise<string[]> {
  const { gitDir } = await gitDirs(repo);
  return repo.exclusive(async (run) => {
    const [blocker, hasHead] = await Promise.all([
      underWayBlocker(gitDir, run, "discard the changes"),
      repo.hasHead(),
    ]);
    if (blocker) throw new DiscardBlockedError(blocker);
    // First, so what's in the way of a deleted file is only what it leaves.
    await run(["clean", "-f", "-d", "-q"], REWRITES);
    if (hasHead) {
      await restoreAll(repo, run);
    } else {
      // Before the first commit, everything staged is new.
      await run(["rm", "-r", "-f", "-q", "--ignore-unmatch", "--", "."], REWRITES);
    }
    return parseStatus(await run(STATUS_ARGS)).files.map((file) => file.path);
  });
}

/**
 * Puts every tracked file back as HEAD has it, through `run` (a command of `repo.exclusive`), but
 * the deleted ones something has taken the place of (see `discardAll`).
 */
async function restoreAll(repo: Repo, run: GitCommand): Promise<void> {
  // As pathspecs, with their magic, and as bytes: a path goes back to git as it came, also one
  // that isn't UTF-8.
  const options = { globPathspecs: true, binary: true, ...REWRITES };
  const restore = (pathspecs: string[]) =>
    run(["restore", "--source=HEAD", "--staged", "--worktree", ...PATHS_FROM_STDIN], {
      stdin: nulSeparated(pathspecs),
      ...options,
    });
  const inTheWay = async (paths: string[]) => {
    const entries = await entriesAt(
      repo.path,
      paths.map((path) => Buffer.from(path, "latin1")),
    );
    return paths.filter((_, i) => entries[i] !== undefined);
  };
  const deleted = await run(
    ["diff", "HEAD", "--name-only", "-z", "--no-renames", "--diff-filter=D"],
    options,
  );
  const kept = await inTheWay(deleted.split("\0").filter(Boolean));
  try {
    // Left out by themselves, not with what's in a folder of the same name, which can be staged
    // files to discard: as a glob that matches the path alone, every character escaped.
    await restore([":/", ...kept.map((path) => `:(exclude,glob)${path.replace(/[^/]/g, "\\$&")}`)]);
  } catch (error) {
    // Nothing's tracked, in HEAD nor in the index: there's nothing to restore.
    if (error instanceof GitError && /did not match any file/.test(error.stderr)) return;
    throw error;
  }
  // Those whose way was cleared, by discarding the staged files in the folder that took their place.
  const cleared = new Set(await inTheWay(kept));
  const freed = kept.filter((path) => !cleared.has(path));
  if (freed.length) await restore(exactly(freed));
}
