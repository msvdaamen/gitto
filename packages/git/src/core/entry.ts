import { lstat } from "node:fs/promises";
import { join } from "node:path";

/**
 * What's at a path in the working tree: a folder, a file (or a link), nothing (`undefined`), or
 * `blocked`: a file is where one of its folders would go, or it couldn't be looked at.
 */
export type Entry = "folder" | "file" | "blocked" | undefined;

/**
 * What's at `path` in the working tree whose root is `root`; `path` as bytes (see
 * `RunOptions.binary`) if it's a Buffer, as git gave it.
 */
export async function entryAt(root: string, path: string | Buffer): Promise<Entry> {
  const full =
    typeof path === "string" ? join(root, path) : Buffer.concat([Buffer.from(`${root}/`), path]);
  return lstat(full).then(
    (stats) => (stats.isDirectory() ? "folder" : "file"),
    (error: NodeJS.ErrnoException) => (error.code === "ENOENT" ? undefined : "blocked"),
  );
}

/**
 * What's at each of `paths`, as `entryAt` says, read a batch at a time: there can be thousands, as
 * many as the files in a deleted folder. Stops after the batch with one that `until` is true of,
 * if it's given, with only the entries read so far.
 */
export async function entriesAt(
  root: string,
  paths: (string | Buffer)[],
  until?: (entry: Entry) => boolean,
): Promise<Entry[]> {
  const entries: Entry[] = [];
  for (let i = 0; i < paths.length; i += 100) {
    const batch = paths.slice(i, i + 100);
    // oxlint-disable-next-line no-await-in-loop -- one batch at a time, on purpose.
    const read = await Promise.all(batch.map((path) => entryAt(root, path)));
    entries.push(...read);
    if (until && read.some(until)) break;
  }
  return entries;
}
