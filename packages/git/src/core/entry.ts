import { lstat, rmdir, unlink } from "node:fs/promises";
import { join } from "node:path";

/**
 * What's at a path in the working tree: a folder, a file, a link, something else (like a pipe),
 * nothing (`undefined`), `blocked` (a file or a link is where one of its folders would go), or
 * `unreadable` (it couldn't be looked at, like in a folder the user can't read).
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

/** What a path's folder is: a folder, nothing, something else, or what couldn't be looked at. */
type FolderEntry = "folder" | "missing" | "other" | "unreadable";

/**
 * What's at `path` in the working tree whose root is `root` (see `fullPath`). Its folders are looked
 * at first, from the top, each once for all the paths `folders` is shared by: a link to a folder,
 * which `lstat` would go through, is no folder of the working tree's.
 */
export async function entryAt(
  root: string,
  path: string | Buffer,
  folders = new Map<string, Promise<FolderEntry>>(),
): Promise<Entry> {
  const asBytes = typeof path !== "string";
  const parts = (asBytes ? path.toString("latin1") : path).split("/");
  for (let depth = 1; depth < parts.length; depth++) {
    const folder = parts.slice(0, depth).join("/");
    let entry = folders.get(folder);
    if (!entry) {
      entry = lstat(fullPath(root, asBytes ? Buffer.from(folder, "latin1") : folder)).then(
        (stats) => (stats.isDirectory() ? "folder" : "other"),
        (error: NodeJS.ErrnoException) => {
          if (error.code === "ENOENT") return "missing";
          return error.code === "ENOTDIR" ? "other" : "unreadable";
        },
      );
      folders.set(folder, entry);
    }
    // oxlint-disable-next-line no-await-in-loop -- a folder at a time, from the top.
    const kind = await entry;
    if (kind === "missing") return undefined;
    if (kind === "unreadable") return "unreadable";
    if (kind === "other") return "blocked";
  }
  return lstat(fullPath(root, path)).then(
    (stats) => {
      if (stats.isDirectory()) return "folder";
      if (stats.isFile()) return "file";
      return stats.isSymbolicLink() ? "link" : "other";
    },
    (error: NodeJS.ErrnoException) => (error.code === "ENOENT" ? undefined : "unreadable"),
  );
}

/**
 * What's at each of `paths`, as `entryAt` says, a batch at a time, their folders looked at once.
 * Stops after the batch with one that `until` is true of, if it's given, with only the entries read
 * so far.
 */
export function entriesAt(
  root: string,
  paths: (string | Buffer)[],
  until?: (entry: Entry) => boolean,
): Promise<Entry[]> {
  const folders = new Map<string, Promise<FolderEntry>>();
  return inBatches(paths, (path) => entryAt(root, path, folders), until);
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
