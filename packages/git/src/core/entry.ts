import { lstat, rmdir, unlink } from "node:fs/promises";
import { join } from "node:path";

/**
 * What's at a path in the working tree: a folder, a file, a link, something else (like a pipe),
 * nothing (`undefined`), `blocked` (a file is where one of its folders would go), or `unreadable`
 * (it couldn't be looked at, like in a folder the user can't read).
 */
export type Entry = "folder" | "file" | "link" | "other" | "blocked" | "unreadable" | undefined;

/**
 * Where `path` is, in the working tree whose root is `root`; `path` as bytes (see
 * `RunOptions.binary`) if it's a Buffer, as git gave it.
 */
export function fullPath(root: string, path: string | Buffer): string | Buffer {
  return typeof path === "string"
    ? join(root, path)
    : Buffer.concat([Buffer.from(`${root}/`), path]);
}

/** `task` for each of `items`, a batch at a time: there can be thousands, too many at once. */
export async function inBatches<T, R>(
  items: T[],
  task: (item: T) => Promise<R>,
  until?: (result: R) => boolean,
): Promise<R[]> {
  const results: R[] = [];
  for (let i = 0; i < items.length; i += 100) {
    // oxlint-disable-next-line no-await-in-loop -- one batch at a time, on purpose.
    const batch = await Promise.all(items.slice(i, i + 100).map(task));
    results.push(...batch);
    if (until && batch.some(until)) break;
  }
  return results;
}

/** What's at `path` in the working tree whose root is `root` (see `fullPath`). */
export async function entryAt(root: string, path: string | Buffer): Promise<Entry> {
  return lstat(fullPath(root, path)).then(
    (stats) => {
      if (stats.isDirectory()) return "folder";
      if (stats.isFile()) return "file";
      return stats.isSymbolicLink() ? "link" : "other";
    },
    async (error: NodeJS.ErrnoException) => {
      // On Windows, a file where one of its folders goes is ENOENT too, rather than ENOTDIR.
      if (error.code === "ENOENT") return (await fileAbove(root, path)) ? "blocked" : undefined;
      return error.code === "ENOTDIR" ? "blocked" : "unreadable";
    },
  );
}

/** Whether something other than a folder is where one of `path`'s folders goes. */
async function fileAbove(root: string, path: string | Buffer): Promise<boolean> {
  const name = typeof path === "string" ? path : path.toString("latin1");
  const slash = name.lastIndexOf("/");
  if (slash === -1) return false;
  const parent = name.slice(0, slash);
  const folder = typeof path === "string" ? parent : Buffer.from(parent, "latin1");
  return lstat(fullPath(root, folder)).then(
    (stats) => !stats.isDirectory(),
    (error: NodeJS.ErrnoException) =>
      error.code === "ENOENT" ? fileAbove(root, folder) : error.code === "ENOTDIR",
  );
}

/**
 * What's at each of `paths`, as `entryAt` says, a batch at a time. Stops after the batch with one
 * that `until` is true of, if it's given, with only the entries read so far.
 */
export function entriesAt(
  root: string,
  paths: (string | Buffer)[],
  until?: (entry: Entry) => boolean,
): Promise<Entry[]> {
  return inBatches(paths, (path) => entryAt(root, path), until);
}

/**
 * Deletes the files and links at `paths` (as bytes) in the working tree whose root is `root`, a
 * batch at a time, then the folders that leaves empty, as git keeps none. Resolves to the paths it
 * couldn't delete.
 */
export async function deleteFiles(root: string, paths: Buffer[]): Promise<Buffer[]> {
  const deleted = await inBatches(paths, (path) =>
    unlink(fullPath(root, path)).then(
      () => true,
      (error: NodeJS.ErrnoException) => error.code === "ENOENT",
    ),
  );
  // The folders they were in, by depth, the deepest first: one empties another.
  const byDepth: Set<string>[] = [];
  for (const path of paths) {
    const parts = path.toString("latin1").split("/");
    for (let depth = parts.length - 1; depth > 0; depth--) {
      (byDepth[depth] ??= new Set()).add(parts.slice(0, depth).join("/"));
    }
  }
  for (const folders of byDepth.toReversed()) {
    if (!folders) continue;
    // oxlint-disable-next-line no-await-in-loop -- a depth at a time, on purpose.
    await inBatches([...folders], (folder) =>
      // Fails, as it should, at one that isn't empty.
      rmdir(fullPath(root, Buffer.from(folder, "latin1"))).catch(() => undefined),
    );
  }
  return paths.filter((_, i) => !deleted[i]);
}
