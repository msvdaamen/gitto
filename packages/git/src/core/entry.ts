import { lstat } from "node:fs/promises";
import { join } from "node:path";

/**
 * What's at `path` in the working tree whose root is `root`: a folder, a file (or a link), nothing,
 * or `blocked`: a file is where one of its folders would go, or git couldn't look. `path` as bytes
 * (see `RunOptions.binary`) if it's a Buffer, as git gave it.
 */
export async function entryAt(
  root: string,
  path: string | Buffer,
): Promise<"folder" | "file" | "blocked" | undefined> {
  const full =
    typeof path === "string" ? join(root, path) : Buffer.concat([Buffer.from(`${root}/`), path]);
  return lstat(full).then(
    (stats) => (stats.isDirectory() ? "folder" : "file"),
    (error: NodeJS.ErrnoException) => (error.code === "ENOENT" ? undefined : "blocked"),
  );
}
