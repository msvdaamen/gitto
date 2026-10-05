import { constants } from "node:fs";
import { open, realpath } from "node:fs/promises";
import { isAbsolute, join, relative, sep } from "node:path";

import { FileTooLargeError, NotUtf8Error, OutsideRepositoryError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { MAX_BLOB_BYTES } from "./commands";

/**
 * Where the file at `path` (relative to the repository, as git names it) is on disk, links
 * followed. Rejects with `OutsideRepositoryError` for a path that leads out of the working tree,
 * with `..` or through a link, or into the git directory.
 */
export async function resolveWorkingTreePath(repo: Repo, path: string): Promise<string> {
  const parts = path.split(/[\\/]/);
  if (!path || isAbsolute(path) || parts.some((part) => part === ".." || part === ".git")) {
    throw new OutsideRepositoryError(path);
  }
  const root = await realpath(repo.path);
  const resolved = await realpath(join(root, path)).catch(() => {
    throw new OutsideRepositoryError(path);
  });
  const inside = relative(root, resolved);
  if (!inside || inside.startsWith(`..${sep}`) || isAbsolute(inside)) {
    throw new OutsideRepositoryError(path);
  }
  return resolved;
}

/**
 * The contents of a file in the working tree, as they are on disk: to show more of its unstaged
 * changes, which git only has a patch of. Rejects with `FileTooLargeError` above `MAX_BLOB_BYTES`,
 * like a blob, and with `NotUtf8Error` for one that isn't UTF-8.
 */
export async function readWorkingTreeFile(repo: Repo, path: string): Promise<string> {
  const resolved = await resolveWorkingTreePath(repo, path);
  // Not following a link put in its place since: it was checked above.
  const file = await open(resolved, constants.O_RDONLY | constants.O_NOFOLLOW);
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
