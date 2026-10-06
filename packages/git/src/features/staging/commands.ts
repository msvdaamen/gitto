import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { access, readdir, readlink } from "node:fs/promises";
import { join, sep } from "node:path";

import { deleteFiles, entriesAt, entryAt, fullPath, inBatches, type Entry } from "../../core/entry";
import {
  DiscardBlockedError,
  GitError,
  LinesNotStageableError,
  PatchChangedError,
} from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import { resolveRef, type GitCommand, type Repo } from "../../core/repo";
import type { FileStatus } from "../../schema";
import { refuseConflictMarkers } from "../conflicts/commands";
import { emptyTree, getStagedFilePatch, getUnstagedFilePatch } from "../diff/commands";
import { checkWorkingTreePath } from "../diff/working-tree";
import { operationBlocker, underWayBlocker } from "../operation/commands";
import { linesPatch, type LineAction } from "./lines";
import type { KeptChange, LineSelection, UncommittedSide } from "./schema";

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

/**
 * `restore`, not into submodules, whatever `submodule.recurse` says: their changes are their own,
 * and kept.
 */
const RESTORE = ["restore", "--no-recurse-submodules"];

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
 * A pathspec leaving out `path` alone, not what's in a folder of the same name: a glob that matches
 * only it, as one with a wildcard does. Its glob characters, and one other, are each put in a
 * bracket of their own, rather than escaped with a backslash, which git on Windows reads as a
 * slash. A name with nothing a bracket can hold is left out with what's in it.
 */
function excludingExactly(path: string): string {
  const chars = [...path];
  // Not `!` or `^`, which would negate the bracket, nor a slash.
  const last = chars.findLastIndex((char) => !"/!^\\".includes(char));
  if (last === -1) return `:(exclude,literal)${path}`;
  const glob = chars.map((char, i) => {
    if (i === last || "*?[".includes(char)) return `[${char}]`;
    // Only in a name outside Windows, where it's no slash.
    return char === "\\" ? "\\\\" : char;
  });
  return `:(exclude,glob)${glob.join("")}`;
}

/**
 * Whether what's at a rename's previous path, `entry`, is only the rename's own `path`: a folder
 * that holds nothing else, where it was renamed into a folder of its previous name, or the file
 * itself where one of its previous path's folders goes, where it was renamed out of one.
 */
async function ownRenameInTheWay(
  repo: Repo,
  path: string,
  origPath: string,
  entry: Entry,
): Promise<boolean> {
  if (entry === "blocked") return origPath.startsWith(`${path}/`);
  if (entry !== "folder" || !path.startsWith(`${origPath}/`)) return false;
  // The folders down to it, and it.
  const inner = path.slice(origPath.length + 1).split("/");
  const own = new Set(inner.map((_, i) => inner.slice(0, i + 1).join(sep)));
  const held = await readdir(join(repo.path, origPath), { recursive: true });
  return held.every((each) => own.has(each));
}

/** What's in the way of putting back the file at `path`, where `entry` is. */
function inTheWayOf(path: string, entry: Entry): string {
  switch (entry) {
    case "folder":
      return `A folder is at ${path} now.`;
    case "blocked":
      return `A file is where a folder of ${path} goes.`;
    case "other":
      return `Something that isn't a file is at ${path} now.`;
    case "unreadable":
      return `${path} can't be looked at, as one of its folders can't be read.`;
    default:
      // Only ever one put back from the source, on the staged side.
      return `The file at ${path} isn't the last commit's.`;
  }
}

/** The tree changes are discarded back to: HEAD's, or before the first commit, the empty tree. */
async function sourceTree(run: GitCommand): Promise<string> {
  return (await resolveRef(run, "HEAD")) ?? emptyTree(run);
}

/**
 * Those of `paths` (as bytes if `binary`) whose file or link in the working tree is as `source` has
 * it: writing it over loses nothing, like a file's own copy left by `rm --cached`.
 */
async function unchangedSince(
  repo: Repo,
  run: GitCommand,
  source: string,
  paths: { path: string; entry: Entry }[],
  options: { binary?: boolean } = {},
): Promise<Set<string>> {
  const bytes = (path: string) => Buffer.from(path, options.binary ? "latin1" : "utf8");
  const candidates = paths.filter(({ entry }) => entry === "file").map(({ path }) => path);
  // Not one that can't be read, which `hash-object` would fail at, and all the rest with it: taken
  // for changed, and kept.
  const readable = await inBatches(candidates, (path) =>
    access(fullPath(repo.path, bytes(path)), constants.R_OK).then(
      () => true,
      () => false,
    ),
  );
  const files = candidates.filter((_, i) => readable[i]);
  const links = paths.filter(({ entry }) => entry === "link").map(({ path }) => path);
  const compared = [...files, ...links];
  if (!compared.length) return new Set();
  const [sources, hashes, targets] = await Promise.all([
    run(["cat-file", "--batch-check=%(objectname)", "-z"], {
      ...options,
      stdin: nulSeparated(compared.map((path) => `${source}:${path}`)),
    }),
    files.length
      ? // A line each, quoted as git reads a line that starts with a quote: a path can have any
        // character, a newline too.
        run(["hash-object", "--stdin-paths"], {
          ...options,
          stdin: files
            .map((path) => `"${path.replace(/[\\"]/g, "\\$&").replace(/\n/g, "\\n")}"\n`)
            .join(""),
        }).then((output) => output.split("\n").slice(0, files.length))
      : [],
    inBatches(links, (path) => readlink(fullPath(repo.path, bytes(path)), { encoding: "buffer" })),
  ]);
  // What the source has at each, a line each: its object's name, or for one it doesn't have, what
  // was asked for and "missing", which can be lines of its own.
  const objects: (string | undefined)[] = [];
  let at = 0;
  for (const path of compared) {
    const missing = `${source}:${path} missing\n`;
    if (sources.startsWith(missing, at)) {
      objects.push(undefined);
      at += missing.length;
    } else {
      const end = sources.indexOf("\n", at);
      objects.push(sources.slice(at, end));
      at = end + 1;
    }
  }
  // A link's blob is where it leads, which `hash-object` would follow instead: hashed as git does,
  // with the repository's hash, which the source's name is as long as.
  const algorithm = source.length === 64 ? "sha256" : "sha1";
  const ours = [
    ...hashes,
    ...targets.map((target) =>
      createHash(algorithm).update(`blob ${target.length}\0`).update(target).digest("hex"),
    ),
  ];
  return new Set(compared.filter((_, i) => objects[i] !== undefined && objects[i] === ours[i]));
}

/**
 * Discards a file's changes on `side`, the side of the uncommitted changes it's listed on; a
 * renamed one's at both its paths. A copy's source is another file, and left as it is. From its
 * unstaged changes, those: a tracked file goes back to how the index has it, and an untracked one,
 * or one added with `--intent-to-add`, is deleted. From its staged changes, all of them: it goes
 * back to how HEAD has it, in the index and the working tree, and one HEAD doesn't have is deleted;
 * before the first commit, all of them are.
 *
 * Rejects with `DiscardBlockedError`, discarding nothing: for a conflicted file, whose conflicts are
 * resolved instead; for a submodule, whose changes are discarded in it; for a file's staged changes
 * while an operation like a merge is under way, which would go on without them; and where putting
 * a file back would write over something that isn't it: a folder that has taken its place, or
 * another file at a rename's previous path.
 */
export async function discard(
  repo: Repo,
  file: LinesFile & { status: FileStatus },
  side: UncommittedSide,
): Promise<void> {
  const { path, origPath, status } = file;
  const paths = origPath ? [path, origPath] : [path];
  const named = new Set(paths);
  const gitDir = side === "staged" ? (await gitDirs(repo)).gitDir : undefined;
  await repo.exclusive(async (run) => {
    // On the command line, which holds them: there are two at most.
    const listed = async (args: string[]) =>
      (await run([...args, "-z", "--", ...paths])).split("\0").filter(Boolean);
    // Added with `--intent-to-add`: in the index with no contents yet, which `restore` would put
    // back, emptying the file, so it's deleted instead. Staged only when they're shown, deleted from
    // the working tree or not. Without renames, which would pair one with a deleted file.
    const intentToAdd = async () => {
      // `<status>`, then the path: one added with `--intent-to-add` is shown otherwise when it's
      // hidden, as deleted or not at all; any other is shown the same.
      const [shown, hidden] = await Promise.all(
        ["visible", "invisible"].map(async (ita) => {
          const fields = await listed([
            "diff",
            "--cached",
            "--name-status",
            "--no-renames",
            `--ita-${ita}-in-index`,
          ]);
          const statuses = new Map<string, string>();
          for (let i = 0; i + 1 < fields.length; i += 2) statuses.set(fields[i + 1]!, fields[i]!);
          return statuses;
        }),
      );
      return paths.filter((each) => shown!.has(each) && shown!.get(each) !== hidden!.get(each));
    };
    const [entries, intended, source, blocker, there] = await Promise.all([
      // The paths in the index, with their mode and stage: a conflict's are 1-3.
      listed(["ls-files", "--format=%(objectmode) %(stage) %(path)"]),
      side === "unstaged" ? intentToAdd() : ([] as string[]),
      side === "staged" ? sourceTree(run) : "",
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
          `${entryPath} is conflicted. Resolve its conflicts rather than discarding its changes.`,
        );
      }
      if (entry.startsWith("160000")) {
        throw new DiscardBlockedError(
          `${entryPath} is a submodule, a repository of its own: discard its changes in it.`,
        );
      }
      tracked.add(entryPath);
    }
    // The file as its list showed it, tracked or not: otherwise it changed since, and its list is
    // out of date. Discarding what it showed could delete what's tracked now, say.
    const listedTracked = side === "staged" ? status !== "deleted" : status !== "untracked";
    if (listedTracked !== tracked.has(path)) {
      throw new DiscardBlockedError(
        `${path} changed since its changes were shown, so nothing was discarded.`,
      );
    }
    const at = (each: string) => there[paths.indexOf(each)];
    // A copy's source is still there on this side, unlike a rename's: in the index, or in the
    // working tree.
    const copied =
      origPath !== null &&
      (side === "staged"
        ? tracked.has(origPath)
        : at(origPath) === "file" || at(origPath) === "link");
    const discarded = copied ? [path] : paths;
    // Put back as HEAD or the index has them, over what's in the working tree.
    const written =
      side === "staged"
        ? discarded
        : discarded.filter((each) => tracked.has(each) && !intended.includes(each));
    // What's there is fine to write over if it's the tracked file's, or one that's as the source
    // has it, like its copy left by `rm --cached`: nothing's lost. Not a folder, nor another file,
    // like one at a rename's previous path, or one taken out of the index that changed since, which
    // isn't shown as such when it's ignored.
    // Only ever on the staged side, where everything's written from the source.
    const loose = written
      .filter((each) => !tracked.has(each))
      .map((each) => ({ path: each, entry: at(each) }));
    const unchanged = await unchangedSince(repo, run, source, loose);
    for (const each of written) {
      const entry = at(each);
      const files = entry === "file" || entry === "link";
      if (entry === undefined || (files && (tracked.has(each) || unchanged.has(each)))) continue;
      // A rename into a folder of its previous name, or out of one: what's there is the rename's
      // own, which discarding it deletes first.
      // oxlint-disable-next-line no-await-in-loop -- two at most.
      if (each === origPath && !copied && (await ownRenameInTheWay(repo, path, origPath, entry))) {
        continue;
      }
      throw new DiscardBlockedError(
        `${inTheWayOf(each, entry)} Move or delete it, then discard the changes.`,
      );
    }
    if (side === "staged") {
      // One at a time, the file first: where it's in a folder of its previous name, or that's in
      // one of its, it's deleted before that's put back, and the two can't be named apart at once.
      for (const each of discarded) {
        // oxlint-disable-next-line no-await-in-loop -- two at most, one after the other.
        await run(
          [...RESTORE, `--source=${source}`, "--staged", "--worktree", ...PATHS_FROM_STDIN],
          exactlyFromStdin([each]),
        );
      }
      return;
    }
    // Untracked, and deleted as the file that's listed, ignored by now or not. Not a folder: a
    // repository inside this one, whose changes are discarded in it.
    // Gone already, or in a folder a file has taken the place of: nothing's left to delete.
    const untracked = discarded.filter(
      (each) => !tracked.has(each) && at(each) !== undefined && at(each) !== "blocked",
    );
    for (const each of untracked) {
      if (at(each) === "folder") {
        throw new DiscardBlockedError(
          `${each} is a repository of its own: discard its changes in it.`,
        );
      }
      if (at(each) === "unreadable") throw new DiscardBlockedError(inTheWayOf(each, "unreadable"));
      // oxlint-disable-next-line no-await-in-loop -- two at most.
      await checkWorkingTreePath(repo, each);
    }
    const left = await deleteFiles(
      repo.path,
      untracked.map((each) => Buffer.from(each)),
    );
    if (left.length) {
      throw new DiscardBlockedError(`${left[0]!.toString()} couldn't be deleted.`);
    }
    if (intended.length) {
      await run(["rm", "-f", "-q", ...PATHS_FROM_STDIN], exactlyFromStdin(intended));
    }
    if (written.length) {
      await run([...RESTORE, "--worktree", ...PATHS_FROM_STDIN], exactlyFromStdin(written));
    }
  });
}

/**
 * Discards every uncommitted change: tracked files go back to how HEAD has them, in the index and
 * the working tree, and the ones HEAD doesn't have are deleted, staged ones too, ignored or not, as
 * are untracked files. Before the first commit, every staged file is deleted. Ignored files that
 * aren't staged stay, by the ignore files as they are or as they're put back. Resolves to the paths
 * of the changes it keeps, as it can't discard them: a submodule's and a repository's inside this
 * one, which are discarded in them, an untracked file's it couldn't delete, and a deleted file's
 * where putting it back would write over what's taken its place: a folder, a file where one of its
 * folders was, or another file, ignored, like its copy left by `rm --cached` once changed.
 *
 * Rejects with `DiscardBlockedError`, discarding nothing, while files are conflicted, which are
 * resolved instead, or while a merge, rebase, cherry-pick, revert or `git am` is under way: its
 * changes are discarded by aborting it. Kept under way, it would go on without them, and a merge be
 * committed with none of the branch's.
 */
export async function discardAll(repo: Repo): Promise<KeptChange[]> {
  const { gitDir } = await gitDirs(repo);
  return repo.exclusive(async (run) => {
    const [blocker, source] = await Promise.all([
      underWayBlocker(gitDir, run, "discard the changes"),
      sourceTree(run),
    ]);
    if (blocker) throw new DiscardBlockedError(blocker);
    const kept = await restoreAll(repo, run, source);
    return kept.map(({ path, reason }) => ({
      path: Buffer.from(path, "latin1").toString("utf8"),
      reason,
    }));
  });
}

/**
 * Puts every tracked file back as `source` has it, through `run` (a command of `repo.exclusive`),
 * and deletes the untracked ones; resolves to the paths, as bytes, of the changes it keeps (see
 * `discardAll`).
 */
async function restoreAll(repo: Repo, run: GitCommand, source: string): Promise<KeptChange[]> {
  // As pathspecs, with their magic, and as bytes: a path goes back to git as it came, also one
  // that isn't UTF-8.
  const reading = { binary: true };
  const options = { ...reading, globPathspecs: true, ...REWRITES };
  // `args` NUL-separated: `-z` goes before any `--`.
  const list = async (args: string[]) =>
    (await run(args.includes("-z") ? args : [...args, "-z"], reading)).split("\0").filter(Boolean);
  const restore = async (pathspecs: string[]) => {
    try {
      await run([...RESTORE, `--source=${source}`, "--staged", "--worktree", ...PATHS_FROM_STDIN], {
        stdin: nulSeparated(pathspecs),
        ...options,
      });
    } catch (error) {
      // What it names isn't tracked, in the source nor in the index: there's nothing to restore.
      if (!(error instanceof GitError && /did not match any file/.test(error.stderr))) throw error;
    }
  };
  /** Those of the deleted `paths` that something has taken the place of. */
  const inTheWay = async (paths: string[]) => {
    const entries = await entriesAt(
      repo.path,
      paths.map((path) => Buffer.from(path, "latin1")),
    );
    const there = paths.map((path, i) => ({ path, entry: entries[i] }));
    const unchanged = await unchangedSince(repo, run, source, there, reading);
    return paths.filter((path, i) => entries[i] !== undefined && !unchanged.has(path));
  };
  const untracked = () => list(["ls-files", "--others", "--exclude-standard"]);
  const [deleted, before] = await Promise.all([
    list(["diff", source, "--name-only", "--no-renames", "--diff-filter=D"]),
    untracked(),
  ]);
  const kept = await inTheWay(deleted);
  // What's kept is left out by its name alone, not with what's in a folder of the same name, which
  // can be staged files to discard.
  await restore([":/", ...kept.map(excludingExactly)]);
  // Untracked files only once the rest is done, which may fail. Those that aren't ignored, now nor
  // before, by the ignore files as they were: what either ignores isn't listed, like an `.env` a
  // deleted `.gitignore` ignored, or one only an uncommitted rule does. Not a repository inside
  // this one, listed as its folder.
  const listed = new Set(before);
  const after = await untracked();
  const files = after.filter((path) => listed.has(path) && !path.endsWith("/"));
  const left = await deleteFiles(
    repo.path,
    files.map((path) => Buffer.from(path, "latin1")),
  );
  // Those whose way that cleared.
  const still = await inTheWay(kept);
  const freed = kept.filter((path) => !still.includes(path));
  if (freed.length) await restore(exactly(freed));
  // Submodules, whose commits differ: `:<mode> <mode> <object> <object> <status>`, then the path,
  // a submodule's mode on either side, not its objects, which could read the same.
  // The submodules in the index, and of those, the changed ones, whatever `diff.ignoreSubmodules`
  // says: only them, as looking at one runs a status in it.
  const gitlinks = (await list(["ls-files", "--format=%(objectmode) %(path)"]))
    .filter((entry) => entry.startsWith("160000 "))
    .map((entry) => entry.slice(7));
  const submodules = gitlinks.length
    ? await list(["diff", "--name-only", "--ignore-submodules=none", "-z", "--", ...gitlinks])
    : [];
  const repositories = after.filter((path) => path.endsWith("/"));
  // A path once, with the first reason: a repository that took a deleted file's place is in the
  // way, and listed as its folder.
  const told = new Map<string, KeptChange>();
  const keep = (paths: string[], reason: KeptChange["reason"]) => {
    for (const path of paths) {
      const name = path.replace(/\/$/, "");
      if (!told.has(name)) told.set(name, { path, reason });
    }
  };
  keep(still, "in-the-way");
  keep(submodules, "submodule");
  keep(repositories, "repository");
  keep(
    left.map((path) => path.toString("latin1")),
    "undeletable",
  );
  return [...told.values()];
}
