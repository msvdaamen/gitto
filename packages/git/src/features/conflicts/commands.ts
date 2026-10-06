import { lstat } from "node:fs/promises";
import { join } from "node:path";

import {
  ConflictChangedError,
  ConflictMarkersError,
  FileChangedOnDiskError,
  FileTooLargeError,
  NotUtf8Error,
  OutsideRepositoryError,
} from "../../core/errors";
import type { GitCommand, Repo } from "../../core/repo";
import { MAX_BLOB_BYTES } from "../diff/limits";
import { decodeUtf8, readWorkingTreeBytes, scanWorkingTreeFile } from "../diff/working-tree";
import { BINARY_CHECK_BYTES, ConflictCounter, isBinary } from "./markers";
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

/**
 * What's in the working tree at `path`, as `getConflict` has it. Not what a symbolic link leads
 * to, which is no part of the conflict, and can be outside the repository: a link's conflict is
 * resolved by keeping a side.
 */
async function readText(
  repo: Repo,
  path: string,
): Promise<Pick<Conflict, "text" | "version" | "binary" | "unreadable">> {
  const none = { text: null, version: null, binary: false };
  if (await isLink(repo, path)) return { ...none, unreadable: null };
  let file: Awaited<ReturnType<typeof readWorkingTreeBytes>>;
  try {
    file = await readWorkingTreeBytes(repo, path);
  } catch (error) {
    if (error instanceof FileTooLargeError) {
      // Still with a version, which keeping a side checks first: when it last changed, rather
      // than its bytes, which this is read again for whenever the working tree changes.
      const stamp = await stampOf(repo, path);
      const version = stamp === undefined ? null : `${STAMPED}${stamp}`;
      return { ...none, version, unreadable: "This file is too large to show." };
    }
    // In a folder that's a link out of the repository, say.
    if (error instanceof OutsideRepositoryError) return { ...none, unreadable: error.message };
    throw error;
  }
  if (!file) return { ...none, unreadable: null };
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
    if (shown.version !== null) await checkVersion(repo, path, shown.version);
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

/** Starts a version that's when a file last changed (see `stampOf`), not what's in it. */
const STAMPED = "stamp:";

/**
 * Rejects with `FileChangedOnDiskError` if the file at `path` in the working tree isn't at
 * `version` any more, or is gone. Read a piece at a time, however large it is.
 */
async function checkVersion(repo: Repo, path: string, version: string): Promise<void> {
  let now: string | undefined;
  if (version.startsWith(STAMPED)) {
    const stamp = await stampOf(repo, path);
    now = stamp && `${STAMPED}${stamp}`;
  } else {
    now = await scanWorkingTreeFile(repo, path);
  }
  if (now !== version) throw new FileChangedOnDiskError(path, now ? "changed" : "deleted");
}

function sameSides(a: ConflictSides, b: ConflictSides): boolean {
  return sameSide(a.base, b.base) && sameSide(a.ours, b.ours) && sameSide(a.theirs, b.theirs);
}

function sameSide(a: ConflictSide | null, b: ConflictSide | null): boolean {
  return a === null || b === null ? a === b : a.mode === b.mode && a.oid === b.oid;
}

/**
 * Marks the conflicted file at `path` resolved, as it is in the working tree: stages it, or its
 * deletion if it's gone. Rejects with `ConflictMarkersError` if it still has conflict markers,
 * unless `withMarkers`: the user said they belong in it, like a test's fixture of them. Rejects
 * with `FileChangedOnDiskError` if it isn't at `version` (when given), the version the user saw.
 * Does nothing if it isn't conflicted any more, e.g. as it was staged in a terminal.
 */
export async function markResolved(
  repo: Repo,
  path: string,
  version: string | null,
  withMarkers = false,
): Promise<void> {
  await repo.exclusive(async (run) => {
    const sides = await readSides(run, path);
    if (!sides.base && !sides.ours && !sides.theirs) return;
    if (version !== null) await checkVersion(repo, path, version);
    if (!withMarkers) await refuseConflictMarkers(repo, run, [path]);
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
  // However large it is: only one that's left out could be staged with its markers.
  const marked = await readAtOnce(staged, async (path) =>
    ((await conflictsIn(repo, path, Infinity)) ?? 0) > 0 ? path : undefined,
  );
  const left = marked.filter((path) => path !== undefined);
  if (left.length > 0) throw new ConflictMarkersError(left);
}

/**
 * How many conflicted files are read at once, to tell whether they have markers left: a merge can
 * leave thousands conflicted, and every one of them open at the same time would run the process
 * out of file descriptors.
 */
const READ_AT_ONCE = 50;

/** `read` of each of `paths`, `READ_AT_ONCE` at a time, in order. */
async function readAtOnce<T>(paths: string[], read: (path: string) => Promise<T>): Promise<T[]> {
  const results: T[] = [];
  for (let i = 0; i < paths.length; i += READ_AT_ONCE) {
    // oxlint-disable-next-line no-await-in-loop -- one batch at a time, on purpose.
    results.push(...(await Promise.all(paths.slice(i, i + READ_AT_ONCE).map(read))));
  }
  return results;
}

/**
 * How many conflicts the text file at `path` in the working tree has left; `undefined` if it isn't
 * one: it's gone, binary, larger than `maxBytes`, or a symbolic link, which git doesn't write
 * markers into, and which can lead anywhere.
 */
async function conflictsIn(
  repo: Repo,
  path: string,
  maxBytes: number,
): Promise<number | undefined> {
  if (await isLink(repo, path)) return undefined;
  const counter = new ConflictCounter();
  let read = 0;
  let binary = false;
  let version: string | undefined;
  try {
    // A piece at a time: a conflicted file can be of any size.
    version = await scanWorkingTreeFile(
      repo,
      path,
      (chunk) => {
        if (read < BINARY_CHECK_BYTES && isBinary(chunk.subarray(0, BINARY_CHECK_BYTES - read))) {
          binary = true;
        }
        read += chunk.length;
        // Markers are ASCII, which reads the same in any of the encodings git merges as text.
        if (!binary) counter.push(chunk.toString("latin1"));
      },
      maxBytes,
    );
  } catch (error) {
    if (error instanceof FileTooLargeError || error instanceof OutsideRepositoryError) {
      return undefined;
    }
    throw error;
  }
  return version === undefined || binary ? undefined : counter.end();
}

/** Whether `path` in the working tree is a symbolic link; not if there's nothing there. */
function isLink(repo: Repo, path: string): Promise<boolean> {
  return lstat(join(repo.path, path)).then(
    (stats) => stats.isSymbolicLink(),
    () => false,
  );
}

/** When a file in the working tree last changed, going by what the file system says of it. */
async function stampOf(repo: Repo, path: string): Promise<string | undefined> {
  const stats = await lstat(join(repo.path, path)).catch(() => undefined);
  return stats && `${stats.ino}:${stats.size}:${stats.mtimeMs}:${stats.ctimeMs}`;
}

/**
 * Each repository's conflicted files as `markerFree` last read them, by their paths: whether they
 * had markers left, and when they last changed then. Only the ones asked about last are kept.
 */
const markerChecks = new Map<string, Map<string, { stamp: string; free: boolean }>>();

/**
 * Which of the conflicted text files at `paths` have no conflict markers left: resolved in the
 * working tree, but not marked so yet. Not one that's binary, which git leaves without markers, nor
 * one too large to read (see `MAX_BLOB_BYTES`), or gone. Asked on every change in the working tree
 * (see `getStatus`), so a file is only read again once it changed since.
 */
export async function markerFree(repo: Repo, paths: string[]): Promise<string[]> {
  const last = markerChecks.get(repo.path);
  const checks = new Map<string, { stamp: string; free: boolean }>();
  const free = await readAtOnce(paths, async (path) => {
    const stamp = await stampOf(repo, path);
    const known = stamp === undefined ? undefined : last?.get(path);
    const isFree =
      known !== undefined && known.stamp === stamp
        ? known.free
        : (await conflictsIn(repo, path, MAX_BLOB_BYTES).catch(() => undefined)) === 0;
    if (stamp !== undefined) checks.set(path, { stamp, free: isFree });
    return isFree ? path : undefined;
  });
  markerChecks.set(repo.path, checks);
  return free.filter((path) => path !== undefined);
}
