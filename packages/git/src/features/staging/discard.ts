import { lstat } from "node:fs/promises";
import { join } from "node:path";

import { DiscardBlockedError, RepositoryChangedError } from "../../core/errors";
import { gitDirs } from "../../core/git-dirs";
import { hasConflicts, refExists, succeeds, type GitCommand, type Repo } from "../../core/repo";
import type { FileStatus } from "../../schema";
import { OPERATION_NAMES, operationUnderWay } from "../operation/commands";
import { nulSeparated, PATHS_FROM_STDIN, unstageArgs } from "./commands";

/** A file whose changes are discarded, as its list has it. */
interface DiscardedFile {
  path: string;
  /** Its previous path, if it's renamed; a copy's is left alone, as that file is still there. */
  origPath: string | null;
  status: FileStatus;
}

/** A file in the index or in HEAD, at one of the paths whose changes are discarded or below it. */
interface Entry {
  path: string;
  mode: string;
  /** Its stage in the index: other than `0` for a conflicted file's sides. */
  stage: string;
}

/** The files whose changes are discarded by deleting them, as their list has them. */
const DELETED_STATUSES: ReadonlySet<FileStatus> = new Set([
  "untracked",
  "added",
  "copied",
  "renamed",
]);

/**
 * Discards a file's changes on one side of the uncommitted ones, `side`, a renamed one's by both
 * its paths. Its unstaged changes put it back as the index has it, deleting it if it's untracked or
 * only marked to be added (`add -N`). Its staged ones put it back as HEAD has it, on disk too, so its
 * unstaged ones go with them; one HEAD doesn't have is deleted, even if it's ignored.
 *
 * Rejects with `DiscardBlockedError`, discarding nothing: if it's conflicted, as what to keep of
 * that is for resolving it; if it's a submodule or another repository inside this one, whose
 * changes are its own; or if putting it back would also lose another file, like an untracked one at
 * the old path of a rename, or the files in a folder that replaced it. Rejects with
 * `RepositoryChangedError` if it changed since its list was read, so that another file than the
 * user was told would be deleted, or one they weren't told would be.
 */
export async function discard(
  repo: Repo,
  file: DiscardedFile,
  side: "staged" | "unstaged",
): Promise<void> {
  // Git lists a repository inside this one as an untracked folder.
  if (file.path.endsWith("/")) {
    throw new DiscardBlockedError(
      `${file.path} is a repository of its own, so its changes weren't discarded.`,
    );
  }
  const origPath = file.status === "renamed" ? file.origPath : null;
  const paths = origPath ? [file.path, origPath] : [file.path];
  await repo.exclusive(async (run) => {
    // Not `hasHead`, which takes git failing for there being none: the file would be deleted then.
    const hasHead = await refExists(run, "HEAD");
    const [index, head, placeholders] = await Promise.all([
      indexEntries(run, paths),
      hasHead ? headEntries(run, paths) : [],
      side === "unstaged" ? intentToAdd(run, paths) : new Set<string>(),
    ]);
    if (index.some((entry) => entry.stage !== "0")) {
      throw new DiscardBlockedError(
        `${file.path} has conflicts, so its changes weren't discarded. Resolve them, or abort what's under way.`,
      );
    }
    if ([...index, ...head].some((entry) => entry.mode === "160000")) {
      throw new DiscardBlockedError(
        `${file.path} is a submodule, so its changes weren't discarded. Discard them in the submodule itself.`,
      );
    }
    // What the files are put back as: HEAD's for the staged changes, the index's for the unstaged
    // ones, but for a placeholder of a file only marked to be added. Any path without a file there
    // is deleted.
    const kept = side === "staged" ? head : index.filter((entry) => !placeholders.has(entry.path));
    const keptPaths = new Set(kept.map((entry) => entry.path));
    const deleted = paths.filter((path) => !keptPaths.has(path));
    const deletes = DELETED_STATUSES.has(file.status);
    if (
      deleted.includes(file.path) !== deletes ||
      (origPath !== null && deleted.includes(origPath))
    ) {
      throw new RepositoryChangedError(
        `${file.path} changed since it was shown, so its changes weren't discarded.`,
      );
    }
    const inIndex = new Set(index.map((entry) => entry.path));
    const blocker = await inTheWay(repo.path, kept, inIndex, new Set(deleted));
    if (blocker) {
      throw new DiscardBlockedError(
        `${file.path}'s changes weren't discarded: putting it back would also lose ${blocker}.`,
      );
    }

    // Once it's unstaged, the index has the file as HEAD does, which discarding its unstaged
    // changes then puts it back as.
    if (side === "staged") await run(unstageArgs(hasHead), { stdin: nulSeparated(paths) });
    const marked = paths.filter((path) => placeholders.has(path));
    if (marked.length) await run(["rm", "--cached", "-q", "--", ...marked]);
    // First, as a file deleted here can be in the way of a folder put back. Even if it's ignored:
    // one that was force-added is, once it's unstaged.
    if (deleted.length) await run(["clean", "-f", "-x", "-q", "--", ...deleted]);
    if (kept.length) {
      await run(["restore", "--worktree", ...PATHS_FROM_STDIN], {
        stdin: nulSeparated(kept.map((entry) => entry.path)),
        rewritesFiles: true,
      });
    }
  });
}

/** The index's files at `paths`, or below them. */
async function indexEntries(run: GitCommand, paths: string[]): Promise<Entry[]> {
  const output = await run(["ls-files", "--stage", "-z", "--", ...paths]);
  return output
    .split("\0")
    .filter(Boolean)
    .map((line) => {
      // `<mode> <object> <stage>\t<path>`
      const tab = line.indexOf("\t");
      const [mode = "", , stage = ""] = line.slice(0, tab).split(" ");
      return { path: line.slice(tab + 1), mode, stage };
    });
}

/** HEAD's files at `paths`, or below them. */
async function headEntries(run: GitCommand, paths: string[]): Promise<Entry[]> {
  const output = await run(["ls-tree", "-r", "-z", "HEAD", "--", ...paths]);
  return output
    .split("\0")
    .filter(Boolean)
    .map((line) => {
      // `<mode> <type> <object>\t<path>`
      const tab = line.indexOf("\t");
      return { path: line.slice(tab + 1), mode: line.slice(0, line.indexOf(" ")), stage: "0" };
    });
}

/**
 * Which of `paths` are only marked to be added (`add -N`): the index has a placeholder for them,
 * rather than their contents, which git compares the working tree to as a file it doesn't have.
 */
async function intentToAdd(run: GitCommand, paths: string[]): Promise<Set<string>> {
  const fields = (await run(["diff-files", "--raw", "-z", "--", ...paths])).split("\0");
  const added = new Set<string>();
  // `:<old mode> <new mode> <old object> <new object> <status>`, then the path.
  for (let i = 0; i + 1 < fields.length; i += 2) {
    if (fields[i]!.endsWith(" A")) added.add(fields[i + 1]!);
  }
  return added;
}

/**
 * What putting the files `kept` back in the working tree at `root` would lose, besides the changes
 * discarded, if anything: a folder where one goes, or a file in the way of a folder one goes in;
 * or an untracked file where one goes, which `inIndex` doesn't have. Files that are `deleted`
 * anyway don't count.
 */
async function inTheWay(
  root: string,
  kept: Entry[],
  inIndex: ReadonlySet<string>,
  deleted: ReadonlySet<string>,
): Promise<string | undefined> {
  const kinds = new Map<string, Promise<"folder" | "file" | undefined>>();
  const kind = (path: string) => {
    let found = kinds.get(path);
    if (!found) {
      found = lstat(join(root, path)).then(
        (stats) => (stats.isDirectory() ? "folder" : "file"),
        () => undefined,
      );
      kinds.set(path, found);
    }
    return found;
  };
  for (const { path } of kept) {
    const parts = path.split("/");
    for (let depth = 1; depth < parts.length; depth++) {
      const folder = parts.slice(0, depth).join("/");
      // oxlint-disable-next-line no-await-in-loop -- few, and the same folders over and over
      if (!deleted.has(folder) && (await kind(folder)) === "file") return `the file ${folder}`;
    }
    // oxlint-disable-next-line no-await-in-loop
    const here = await kind(path);
    if (here === "folder") return `what's in the folder ${path}`;
    if (here === "file" && !inIndex.has(path)) return `the untracked file ${path}`;
  }
  return undefined;
}

/**
 * Discards every uncommitted change: the index and the working tree are put back as HEAD has them,
 * and untracked files and folders are deleted. Ignored ones are kept, and so are submodules'
 * changes and repositories inside this one. Rejects with `DiscardBlockedError`, discarding nothing:
 * while an operation is under way, like a merge, which the next commit would finish without its
 * changes; while there are conflicts, which this would resolve; or if there's nothing it would
 * discard, but the changes it keeps.
 */
export async function discardAll(repo: Repo): Promise<void> {
  const { gitDir } = await gitDirs(repo);
  await repo.exclusive(async (run) => {
    const [operation, conflicted, hasHead] = await Promise.all([
      operationUnderWay(gitDir),
      hasConflicts(run),
      // Not `hasHead`, which takes git failing for there being none: every file would be deleted.
      refExists(run, "HEAD"),
    ]);
    if (operation) {
      throw new DiscardBlockedError(
        `A ${OPERATION_NAMES[operation]} is under way. Finish or abort it, then discard all changes.`,
      );
    }
    if (conflicted) {
      throw new DiscardBlockedError(
        "Some files have conflicts. Resolve them, then discard all changes.",
      );
    }
    if (!(await hasDiscardableChanges(run, hasHead))) {
      throw new DiscardBlockedError(
        "There's nothing here to discard. A submodule's changes, or those of a repository inside this one, are discarded in it.",
      );
    }
    // Not `reset --hard`, which would also end a merge git left under way in a terminal, say, nor
    // `restore`, which writes every file again, changed or not: in a big repository that's a lot
    // of files touched for nothing.
    await run(
      hasHead
        ? ["read-tree", "-u", "--reset", "HEAD"]
        : ["rm", "-r", "-f", "-q", "--ignore-unmatch", "--", "."],
      { rewritesFiles: true },
    );
    await run(["clean", "-f", "-d", "-q"]);
  });
}

/**
 * Whether `discardAll` has anything to discard: staged or unstaged changes other than submodules',
 * or untracked files it deletes.
 */
async function hasDiscardableChanges(run: GitCommand, hasHead: boolean): Promise<boolean> {
  const [stagedNone, unstagedNone, untracked] = await Promise.all([
    hasHead
      ? succeeds(run, ["diff-index", "--cached", "--quiet", "--ignore-submodules", "HEAD"])
      : run(["ls-files"]).then((files) => files === ""),
    succeeds(run, ["diff-files", "--quiet", "--ignore-submodules"]),
    run(["clean", "-n", "-d"]),
  ]);
  return !stagedNone || !unstagedNone || untracked !== "";
}
