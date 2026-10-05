import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { lstat, open, realpath, rename, rm, stat } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, sep } from "node:path";

import {
  FileChangedOnDiskError,
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

/** A file in the working tree as it was read, and `version`, which names its bytes. */
export interface WorkingTreeFile {
  contents: string;
  /** Changes whenever the file's bytes do: what a save checks the file still has. */
  version: string;
}

/**
 * The contents of a file in the working tree, as they are on disk: to show more of its unstaged
 * changes, which git only has a patch of, and to edit it. Rejects with `FileTooLargeError` above
 * `MAX_BLOB_BYTES`, like a blob, and with `NotUtf8Error` for one that isn't UTF-8.
 */
export async function readWorkingTreeFile(repo: Repo, path: string): Promise<WorkingTreeFile> {
  const bytes = await readBytes(await resolveWorkingTreePath(repo, path), path);
  return { contents: decodeUtf8(bytes), version: versionOf(bytes) };
}

/** The bytes of the file at `resolved` (the path `path` leads to). */
async function readBytes(resolved: string, path: string): Promise<Buffer> {
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
    return await file.readFile();
  } finally {
    await file.close();
  }
}

function versionOf(bytes: Uint8Array): string {
  return createHash("sha1").update(bytes).digest("hex");
}

/**
 * Saves `contents` as the file at `path` in the working tree, which has to be as it was read
 * (`version`) unless `overwrite`: rejects with `FileChangedOnDiskError` otherwise, or with
 * `WorkingTreeFileNotFoundError` once it's gone. Only for a save the user asked for: Gitto doesn't
 * write to the working tree on its own.
 *
 * The file keeps its permissions, and its CRLF line ends if it has them throughout: the edits can
 * have LF ones, as git shows a file with `core.autocrlf` in its patches. Written to a file next to
 * it, then put in its place, so it's never found half written. A link isn't saved through, nor is a
 * file that isn't UTF-8. Resolves to the saved file's version.
 */
export async function saveWorkingTreeFile(
  repo: Repo,
  path: string,
  contents: string,
  expected: { version: string; overwrite: boolean },
): Promise<string> {
  const resolved = await resolveWorkingTreePath(repo, path);
  if ((await lstat(join(await realpath(repo.path), path))).isSymbolicLink()) {
    throw new OutsideRepositoryError(path);
  }
  const before = await readBytes(resolved, path);
  const text = decodeUtf8(before);
  if (!expected.overwrite && versionOf(before) !== expected.version) {
    throw new FileChangedOnDiskError(path);
  }
  const bytes = Buffer.from(hasCrlfLineEnds(text) ? toCrlf(contents) : contents, "utf8");
  const { mode } = await stat(resolved);
  const temporary = join(dirname(resolved), `.${basename(resolved)}.${randomUUID()}.gitto`);
  try {
    const file = await open(temporary, "wx", mode & 0o7777);
    try {
      await file.writeFile(bytes);
      // Permissions as they were, whatever the umask took off when it was made.
      await file.chmod(mode & 0o7777);
      await file.sync();
    } finally {
      await file.close();
    }
    await rename(temporary, resolved);
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
  return versionOf(bytes);
}

/** Whether every line of `text` but its last ends in CRLF. */
function hasCrlfLineEnds(text: string): boolean {
  return text.includes("\r\n") && !/(^|[^\r])\n/.test(text);
}

/** `text` with its LF line ends made CRLF, and the CRLF ones left alone. */
function toCrlf(text: string): string {
  return text.replace(/\r?\n/g, "\r\n");
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
