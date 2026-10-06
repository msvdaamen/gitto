import { lstat } from "node:fs/promises";
import { join } from "node:path";

import {
  ConflictChangedError,
  ConflictMarkersError,
  FileChangedOnDiskError,
  FileTooLargeError,
  NotUtf8Error,
} from "../../core/errors";
import type { GitCommand, Repo } from "../../core/repo";
import { decodeUtf8, readWorkingTreeBytes } from "../diff/working-tree";
import { countConflicts, isBinary } from "./markers";
import type { Conflict, ConflictSide, ConflictSides } from "./schema";

/** A submodule's mode in the index: its commit, rather than a file. */
const SUBMODULE = "160000";

/**
 * The sides of the conflicted file at `path`, as `git ls-files -u` lists them: stage 1 is the
 * common ancestor's, 2 ours, 3 theirs. All `null` once it isn't conflicted.
 */
export async function readSides(run: GitCommand, path: string): Promise<ConflictSides> {
  const sides: ConflictSides = { base: null, ours: null, theirs: null };
  // `<mode> <object> <stage>\t<path>`; a folder's would list the files in it, which aren't it.
  for (const entry of (await run(["ls-files", "-u", "-z", "--", path])).split("\0")) {
    const tab = entry.indexOf("\t");
    if (tab === -1 || entry.slice(tab + 1) !== path) continue;
    const [mode = "", oid = "", stage] = entry.slice(0, tab).split(" ");
    const side: ConflictSide = { mode, oid };
    if (stage === "1") sides.base = side;
    else if (stage === "2") sides.ours = side;
    else if (stage === "3") sides.theirs = side;
  }
  return sides;
}

/** The conflicted paths in the index, e.g. after a merge stopped at conflicts. */
async function unmergedPaths(run: GitCommand): Promise<Set<string>> {
  const paths = new Set<string>();
  // Each path is listed once per side it has.
  for (const entry of (await run(["ls-files", "-u", "-z"])).split("\0")) {
    const tab = entry.indexOf("\t");
    if (tab !== -1) paths.add(entry.slice(tab + 1));
  }
  return paths;
}

/**
 * The conflicted file at `path`: its sides, and the file in the working tree, as text to resolve
 * its conflicts in if it's text, with its version, which writing it checks it still has.
 */
export async function getConflict(repo: Repo, path: string): Promise<Conflict> {
  const [sides, file] = await Promise.all([readSides(repo.read, path), readText(repo, path)]);
  return { ...sides, ...file };
}

/** What's in the working tree at `path`, as `getConflict` has it. */
async function readText(
  repo: Repo,
  path: string,
): Promise<Pick<Conflict, "text" | "version" | "binary" | "unreadable">> {
  let file: Awaited<ReturnType<typeof readWorkingTreeBytes>>;
  try {
    file = await readWorkingTreeBytes(repo, path);
  } catch (error) {
    if (!(error instanceof FileTooLargeError)) throw error;
    return {
      text: null,
      version: null,
      binary: false,
      unreadable: "This file is too large to show.",
    };
  }
  if (!file) return { text: null, version: null, binary: false, unreadable: null };
  const { bytes, version } = file;
  if (isBinary(bytes)) {
    return { text: null, version, binary: true, unreadable: "This file is binary." };
  }
  try {
    return {
      text: { contents: decodeUtf8(bytes), version },
      version,
      binary: false,
      unreadable: null,
    };
  } catch (error) {
    if (!(error instanceof NotUtf8Error)) throw error;
    return { text: null, version, binary: false, unreadable: error.message };
  }
}

/** What the user saw of a conflict, which resolving it checks is still so. */
export interface ShownConflict {
  sides: ConflictSides;
  /** The version of the file in the working tree, if there was one to check; see `getConflict`. */
  version: string | null;
}

/**
 * Resolves the conflict in the file at `path` by keeping one side whole, `side`, and marks it
 * resolved: the file as that side has it, or deleted if it deleted it. A submodule's commit is
 * only put in the index; its folder is left as it is, to update when the user's ready. Rejects
 * with `ConflictChangedError` if the conflict isn't the one `shown` any more, and with
 * `FileChangedOnDiskError` if the file changed on disk since, which keeping a side would lose.
 */
export async function keepSide(
  repo: Repo,
  path: string,
  side: "ours" | "theirs",
  shown: ShownConflict,
): Promise<void> {
  await repo.exclusive(async (run) => {
    const sides = await readSides(run, path);
    if (!sameSides(sides, shown.sides)) throw new ConflictChangedError(path);
    if (shown.version !== null) {
      // The whole file, however large: only its version matters.
      const file = await readWorkingTreeBytes(repo, path, Infinity);
      if (file?.version !== shown.version) {
        throw new FileChangedOnDiskError(path, file ? "changed" : "deleted");
      }
    }
    const kept = sides[side];
    if (!kept) {
      await run(["rm", "--quiet", "--", path], { rewritesFiles: true });
    } else if (kept.mode === SUBMODULE) {
      await run(["update-index", "--cacheinfo", `${kept.mode},${kept.oid},${path}`]);
    } else {
      await run(["checkout", `--${side}`, "--", path], { rewritesFiles: true });
      await run(["add", "--", path]);
    }
  });
}

function sameSides(a: ConflictSides, b: ConflictSides): boolean {
  return sameSide(a.base, b.base) && sameSide(a.ours, b.ours) && sameSide(a.theirs, b.theirs);
}

function sameSide(a: ConflictSide | null, b: ConflictSide | null): boolean {
  return a === null || b === null ? a === b : a.mode === b.mode && a.oid === b.oid;
}

/**
 * Marks the conflicted file at `path` resolved, as it is in the working tree: stages it, or its
 * deletion if it's gone. Rejects with `ConflictMarkersError` if it still has conflict markers, and
 * with `FileChangedOnDiskError` if it isn't at `version` (when given), the version the user saw.
 * Does nothing if it isn't conflicted any more, e.g. as it was staged in a terminal.
 */
export async function markResolved(
  repo: Repo,
  path: string,
  version: string | null,
): Promise<void> {
  await repo.exclusive(async (run) => {
    const sides = await readSides(run, path);
    if (!sides.base && !sides.ours && !sides.theirs) return;
    if (version !== null) {
      const file = await readWorkingTreeBytes(repo, path, Infinity);
      if (file?.version !== version) {
        throw new FileChangedOnDiskError(path, file ? "changed" : "deleted");
      }
    }
    await refuseConflictMarkers(repo, run, [path]);
    await run(["add", "--all", "--", path]);
  });
}

/**
 * Rejects with `ConflictMarkersError` if staging `paths` (all of the working tree if not given)
 * would mark a conflicted file resolved while it still has conflict markers: git would stage them
 * as they are. Through `run`, a command of the `repo.exclusive` that stages them.
 */
export async function refuseConflictMarkers(
  repo: Repo,
  run: GitCommand,
  paths?: string[],
): Promise<void> {
  const unmerged = [...(await unmergedPaths(run))];
  if (unmerged.length === 0) return;
  // A folder's path stages the files in it.
  const staged = paths
    ? unmerged.filter((path) =>
        paths.some((given) => path === given || path.startsWith(`${given}/`)),
      )
    : unmerged;
  const marked = await Promise.all(
    staged.map(async (path) => ((await conflictsIn(repo, path)) > 0 ? path : undefined)),
  );
  const left = marked.filter((path) => path !== undefined);
  if (left.length > 0) throw new ConflictMarkersError(left);
}

/**
 * How many conflicts the file at `path` in the working tree has left; none if it's binary, gone,
 * or a symbolic link, which git doesn't write markers into, and which can lead anywhere.
 */
async function conflictsIn(repo: Repo, path: string): Promise<number> {
  const isLink = await lstat(join(repo.path, path)).then(
    (stats) => stats.isSymbolicLink(),
    () => false,
  );
  if (isLink) return 0;
  const file = await readWorkingTreeBytes(repo, path, Infinity);
  // Markers are ASCII, which reads the same in any of the encodings git merges as text.
  return file && !isBinary(file.bytes) ? countConflicts(file.bytes.toString("latin1")) : 0;
}

/**
 * Which of the conflicted text files at `paths` have no conflict markers left: resolved in the
 * working tree, but not marked so yet. Not one that's binary, which git leaves without markers, nor
 * one too large to read, or gone.
 */
export async function markerFree(repo: Repo, paths: string[]): Promise<string[]> {
  const free = await Promise.all(
    paths.map(async (path) => {
      const file = await readWorkingTreeBytes(repo, path).catch(() => undefined);
      const resolved =
        file && !isBinary(file.bytes) && countConflicts(file.bytes.toString("latin1")) === 0;
      return resolved ? path : undefined;
    }),
  );
  return free.filter((path) => path !== undefined);
}
