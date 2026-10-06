import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { open, realpath, rename, rm, stat } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, sep } from "node:path";

import {
  FileChangedOnDiskError,
  FileTooLargeError,
  NotUtf8Error,
  OutsideRepositoryError,
  WorkingTreeFileNotFoundError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import { WriteQueue } from "../../core/runner";
import { MAX_BLOB_BYTES } from "./limits";

/**
 * Where the file at `path` (relative to the repository, as git names it) is on disk, links
 * followed. Rejects with `OutsideRepositoryError` for a path that leads out of the working tree
 * (with `..`, or through a link) or into the git directory, and with `WorkingTreeFileNotFoundError`
 * if there's nothing there.
 */
function resolveWorkingTreePath(repo: Repo, path: string): Promise<string> {
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
interface WorkingTreeFile {
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
  const { bytes } = await readFile(await resolveWorkingTreePath(repo, path), path);
  return { contents: decodeUtf8(bytes), version: versionOf(bytes) };
}

/**
 * The bytes of the file at `path` in the working tree, as `readWorkingTreeFile` reads them, and
 * their version; `undefined` if there's no file there: nothing, or a folder, like a submodule's.
 */
export async function readWorkingTreeBytes(
  repo: Repo,
  path: string,
  maxBytes = MAX_BLOB_BYTES,
): Promise<{ bytes: Buffer; version: string } | undefined> {
  const resolved = await resolveFile(repo, path);
  if (!resolved) return undefined;
  const { bytes } = await readFile(resolved, path, maxBytes);
  return { bytes, version: versionOf(bytes) };
}

/** How much of a file `scanWorkingTreeFile` reads at a time. */
const SCAN_CHUNK_BYTES = 1024 * 1024;

/**
 * Reads the file at `path` in the working tree a piece at a time, handing each to `onChunk`, which
 * mustn't keep it: the buffer's used again for the next. Resolves to the file's version (see
 * `readWorkingTreeFile`), never having all of it in memory, whatever its size up to `maxBytes`
 * (`FileTooLargeError` past that); `undefined` if there's no file there.
 */
export async function scanWorkingTreeFile(
  repo: Repo,
  path: string,
  onChunk: (chunk: Buffer) => void = () => undefined,
  maxBytes = Infinity,
): Promise<string | undefined> {
  const resolved = await resolveFile(repo, path);
  if (!resolved) return undefined;
  const file = await open(
    resolved,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  );
  try {
    const stats = await file.stat();
    if (!stats.isFile()) throw new OutsideRepositoryError(path);
    if (stats.size > maxBytes) throw new FileTooLargeError(stats.size);
    const hash = createHash("sha1");
    const buffer = Buffer.alloc(Math.min(SCAN_CHUNK_BYTES, Math.max(stats.size, 1)));
    for (;;) {
      // oxlint-disable-next-line no-await-in-loop -- a piece at a time, in order.
      const { bytesRead } = await file.read(buffer, 0, buffer.length, null);
      if (bytesRead === 0) break;
      const chunk = buffer.subarray(0, bytesRead);
      hash.update(chunk);
      onChunk(chunk);
    }
    return hash.digest("hex");
  } finally {
    await file.close();
  }
}

/** Where the file at `path` is on disk (see `resolveWorkingTreePath`); `undefined` if none is. */
async function resolveFile(repo: Repo, path: string): Promise<string | undefined> {
  try {
    const resolved = await resolveWorkingTreePath(repo, path);
    return (await stat(resolved)).isFile() ? resolved : undefined;
  } catch (error) {
    if (error instanceof WorkingTreeFileNotFoundError) return undefined;
    throw error;
  }
}

/**
 * The bytes and permissions of the file at `resolved` (the path `path` leads to); rejects with
 * `FileTooLargeError` above `maxBytes`.
 */
async function readFile(
  resolved: string,
  path: string,
  maxBytes = MAX_BLOB_BYTES,
): Promise<{ bytes: Buffer; mode: number }> {
  // Not following a link put in its place since it was checked, and not waiting for a writer, as
  // opening a named pipe would: it's no file to read, which is only known once it's open.
  const file = await open(
    resolved,
    constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK,
  );
  try {
    const stats = await file.stat();
    if (!stats.isFile()) throw new OutsideRepositoryError(path);
    if (stats.size > maxBytes) throw new FileTooLargeError(stats.size);
    return { bytes: await file.readFile(), mode: stats.mode };
  } finally {
    await file.close();
  }
}

/** Names `bytes`, the contents of a file in the working tree: what a save checks it still has. */
export function versionOf(bytes: Uint8Array): string {
  return createHash("sha1").update(bytes).digest("hex");
}

/** Saves to the same file, one after the other: a later one checks the file an earlier one wrote. */
const saves = new WriteQueue();

/**
 * The version each file was at when Gitto last saved over it, and the one it saved, by its path
 * on disk: a save over the first is one over the second, as long as nothing else wrote the file
 * since. The last edits are sent as the window closes, without waiting for the version a save
 * under way will leave the file at, which the window won't be there to get.
 */
const savedVersions = new Map<string, { from: string; to: string }>();

/**
 * Saves `contents` as the file at `path` in the working tree, which has to be as it was read
 * (`version`, or one Gitto saved over it since) unless `overwrite`: rejects with
 * `FileChangedOnDiskError` otherwise, or if it's gone, when `overwrite` makes it again. Only for a
 * save the user asked for: Gitto doesn't write to the working tree on its own.
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
  // In the order they're asked for, by the path they're asked for.
  return saves.run(`${repo.path}\0${path}`, async () => {
    // Not following a link at its end, which is refused below.
    const target = await confine(repo, path, false);
    const before = await readExisting(target, path);
    let text = "";
    if (before) {
      text = decodeUtf8(before.bytes);
      const version = versionOf(before.bytes);
      const saved = savedVersions.get(target);
      const unchanged =
        version === expected.version || (saved?.from === expected.version && saved.to === version);
      if (!expected.overwrite && !unchanged) throw new FileChangedOnDiskError(path);
    } else if (!expected.overwrite) {
      throw new FileChangedOnDiskError(path, "deleted");
    }
    const bytes = Buffer.from(hasCrlfLineEnds(text) ? toCrlf(contents) : contents, "utf8");
    await replaceFile(target, bytes, before?.mode);
    const written = versionOf(bytes);
    savedVersions.set(target, { from: expected.version, to: written });
    return written;
  });
}

/**
 * The bytes and permissions of the file at `target` (the path `path` leads to); `undefined` if
 * there's none. Rejects for a link, or anything else that isn't a file.
 */
async function readExisting(
  target: string,
  path: string,
): Promise<{ bytes: Buffer; mode: number } | undefined> {
  try {
    return await readFile(target, path);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
    // `O_NOFOLLOW` refuses a link with ELOOP.
    if ((error as NodeJS.ErrnoException).code === "ELOOP") throw new OutsideRepositoryError(path);
    throw error;
  }
}

/**
 * Puts `bytes` in place of the file at `target`, all at once, with permissions `mode` (the file's
 * own), or those of a new file if there was none.
 */
async function replaceFile(target: string, bytes: Uint8Array, mode?: number): Promise<void> {
  const temporary = join(dirname(target), `.${basename(target)}.${randomUUID()}.gitto`);
  try {
    const file = await open(temporary, "wx", mode === undefined ? 0o666 : mode & 0o7777);
    try {
      await file.writeFile(bytes);
      // Permissions as they were, whatever the umask took off when it was made.
      if (mode !== undefined) await file.chmod(mode & 0o7777);
      await file.sync();
    } finally {
      await file.close();
    }
    await rename(temporary, target);
  } catch (error) {
    await rm(temporary, { force: true });
    throw error;
  }
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
export function decodeUtf8(bytes: Uint8Array): string {
  try {
    return utf8.decode(bytes);
  } catch {
    throw new NotUtf8Error();
  }
}
