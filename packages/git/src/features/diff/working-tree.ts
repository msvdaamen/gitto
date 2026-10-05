import { constants } from "node:fs";
import { open, realpath } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, sep } from "node:path";

import {
  FileTooLargeError,
  NotUtf8Error,
  OutsideRepositoryError,
  WorkingTreeFileNotFoundError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import { MAX_BLOB_BYTES } from "./limits";

/**
 * Where the file at `path` (relative to the repository, as git names it) is on disk, links
 * followed. Rejects with `OutsideRepositoryError` for a path that leads out of the working tree
 * (with `..`, or through a link) or into the git directory, and with `WorkingTreeFileNotFoundError`
 * if there's nothing there.
 */
export function resolveWorkingTreePath(repo: Repo, path: string): Promise<string> {
  return confine(repo, path, true);
}

/**
 * Checks that `path` names something inside the working tree, as `resolveWorkingTreePath` does,
 * but without following a link at its end: for git to read, which reads a link as the path it
 * points to, not the file there.
 */
export async function checkWorkingTreePath(repo: Repo, path: string): Promise<void> {
  await confine(repo, path, false);
}

async function confine(repo: Repo, path: string, followLink: boolean): Promise<string> {
  if (!path || isAbsolute(path) || !isPlain(path)) throw new OutsideRepositoryError(path);
  const root = await realpath(repo.path);
  const full = join(root, path);
  let resolved: string;
  try {
    resolved = followLink
      ? await realpath(full)
      : join(await realpath(dirname(full)), basename(full));
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "ENOENT" || code === "ENOTDIR") throw new WorkingTreeFileNotFoundError(path);
    throw error;
  }
  const inside = relative(root, resolved);
  if (!inside || inside === ".." || inside.startsWith(`..${sep}`) || isAbsolute(inside)) {
    throw new OutsideRepositoryError(path);
  }
  // Also where a link led: into the git directory, say.
  if (!isPlain(inside)) throw new OutsideRepositoryError(path);
  return resolved;
}

/**
 * Whether a relative path stays where it starts and out of git directories: `.git` in any case,
 * as file systems on macOS and Windows don't tell `.GIT` from it.
 */
function isPlain(path: string): boolean {
  return path.split(/[\\/]/).every((part) => part !== ".." && part.toLowerCase() !== ".git");
}

/**
 * The contents of a file in the working tree, as they are on disk: to show more of its unstaged
 * changes, which git only has a patch of. Rejects with `FileTooLargeError` above `MAX_BLOB_BYTES`,
 * like a blob, and with `NotUtf8Error` for one that isn't UTF-8.
 */
export async function readWorkingTreeFile(repo: Repo, path: string): Promise<string> {
  const resolved = await resolveWorkingTreePath(repo, path);
  // Not following a link put in its place since it was checked, and not waiting for a writer, as
  // opening a named pipe would: it's no file to read, which is only known once it's open.
  const file = await open(
    resolved,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  );
  try {
    const stats = await file.stat();
    if (!stats.isFile()) throw new OutsideRepositoryError(path);
    if (stats.size > MAX_BLOB_BYTES) throw new FileTooLargeError(stats.size);
    return decodeUtf8(await file.readFile());
  } finally {
    await file.close();
  }
}

// A byte order mark is kept, as git keeps it in a patch's lines.
const utf8 = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

/** `bytes` as UTF-8; rejects with `NotUtf8Error` if they aren't. */
function decodeUtf8(bytes: Uint8Array): string {
  try {
    return utf8.decode(bytes);
  } catch {
    throw new NotUtf8Error();
  }
}
