import { readdir, rm, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

import { CloneFailedError, GitError } from "../../core/errors";
import { runGit } from "../../core/runner";
import type { CloneProgress } from "./schema";

/**
 * Each folder being cloned into, with when that's over: done, or failed and cleaned up. A clone
 * into the same folder waits for it, e.g. one started again right after cancelling.
 */
const clones = new Map<string, Promise<void>>();

/**
 * Clones the repository at `url` (or a local path, full or from `~`) into `folder`, which must be
 * empty or not there yet: its full path, or one starting with `~` for the home folder. Yields how
 * far it's got as git reports it, only the latest when several come at once, and returns the path
 * it cloned into. If it fails, or is stopped by `signal`, by whatever reads this stopping, or by
 * Gitto exiting, nothing is left of it: git deletes what it cloned, and what git leaves (a
 * repository whose files couldn't all be checked out, say) is deleted once git has exited.
 */
export async function* cloneRepository(
  url: string,
  folder: string,
  signal?: AbortSignal,
): AsyncGenerator<CloneProgress, string> {
  const source = cloneSource(url);
  const path = clonePath(folder);
  // Taken at once, so a clone into the same folder that starts meanwhile waits for this one too.
  const previous = clones.get(path);
  let release!: () => void;
  const turn = new Promise<void>((done) => (release = done));
  clones.set(path, turn);
  void turn.finally(() => {
    if (clones.get(path) === turn) clones.delete(path);
  });

  // Also stopped if whatever reads this stops before the clone is done.
  const stop = new AbortController();
  let latest: CloneProgress | undefined;
  let finished = false;
  let started = false;
  let wake: (() => void) | undefined;
  try {
    await previous;
    const before = await folderState(path);

    // Git rewrites a progress line with `\r` as it goes, and ends it with `\n`; the rest of a
    // line comes with the next chunk.
    let partial = "";
    const onStderr = (text: string) => {
      const lines = (partial + text).split(/[\r\n]/);
      partial = lines.pop()!;
      for (const line of lines) latest = parseProgress(line) ?? latest;
      if (latest) wake?.();
    };
    const clone = runGit(homedir(), ["clone", "--progress", "--", source, path], {
      signal: signal ? AbortSignal.any([signal, stop.signal]) : stop.signal,
      onStderr,
      stopOnExit: true,
      waitForExit: true,
    })
      .catch(async (error: unknown) => {
        await removeClone(path, before);
        throw error;
      })
      .finally(() => {
        finished = true;
        wake?.();
      });
    started = true;
    // Awaited once the progress is read, below; the next clone into the folder waits for it.
    void clone.then(release, release);

    while (true) {
      if (latest) {
        const progress = latest;
        latest = undefined;
        yield progress;
      } else if (finished) {
        break;
      } else {
        // oxlint-disable-next-line no-await-in-loop -- until git says more, or is done.
        await new Promise<void>((woken) => (wake = woken));
      }
    }

    try {
      await clone;
    } catch (error) {
      if (!(error instanceof GitError)) throw error;
      // Only a message when git couldn't start, say.
      throw new CloneFailedError(error.stderr.trim() ? failure(error.stderr) : error.message);
    }
    return path;
  } finally {
    if (!finished) stop.abort();
    if (!started) release();
  }
}

/** `path` with `~` at its start for the home folder. */
function fromHome(path: string): string {
  return /^~(?=$|[\\/])/.test(path) ? homedir() + path.slice(1) : path;
}

/**
 * What `url` is cloned from, as git takes it: a URL (`https://…`, `ssh://…`, `file://…`), a
 * `host:path` as ssh takes it, or a local repository's full path, which can start with `~` for the
 * home folder. Rejects a relative path, which git would take from wherever it runs.
 */
export function cloneSource(url: string): string {
  const trimmed = url.trim();
  const source = fromHome(trimmed);
  const isUrl = /^[a-z][a-z0-9+.-]*:\/\//i.test(source);
  // `host:path` (git's rule: a colon before any slash), but not a Windows drive's `C:\`.
  const isScp = /^[^/\\]+:/.test(source) && !/^[a-z]:[\\/]/i.test(source);
  if (isUrl || isScp || isAbsolute(source)) return source;
  throw new CloneFailedError(
    `${trimmed} isn't a URL, or the full path of a repository. Enter the URL it's cloned from, like https://github.com/owner/repository.git.`,
  );
}

/** `folder` as a full path, with `~` for the home folder; rejects one that isn't a full path. */
export function clonePath(folder: string): string {
  const trimmed = folder.trim();
  const expanded = fromHome(trimmed);
  if (!isAbsolute(expanded)) {
    throw new CloneFailedError(`${trimmed} isn't a full path. Choose the folder to clone into.`);
  }
  return resolve(expanded);
}

/**
 * Whether there's an empty folder at `path` (`empty`), which git clones into, or nothing
 * (`missing`); rejects if there's anything else, which git won't clone into, saying so in words
 * that help in the app.
 */
async function folderState(path: string): Promise<"empty" | "missing"> {
  const inUse = (what: string) =>
    new CloneFailedError(
      `${path} ${what}. Clone into an empty folder, or one that isn't there yet.`,
    );
  let isFolder: boolean;
  try {
    isFolder = (await stat(path)).isDirectory();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return "missing";
    throw new CloneFailedError(`Gitto couldn't clone into ${path}: ${(error as Error).message}`);
  }
  if (!isFolder) throw inUse("is a file");
  if ((await readdir(path)).length > 0) throw inUse("isn't empty");
  return "empty";
}

/**
 * Deletes what a clone that failed left at `path`, which was `before` it started: all of it, or
 * what's in it for a folder that was there, empty. Git deletes it itself when it can, but not when
 * the files couldn't be checked out, nor on Windows, where it's stopped without the chance.
 */
async function removeClone(path: string, before: "empty" | "missing"): Promise<void> {
  try {
    if (before === "missing") {
      await rm(path, { recursive: true, force: true });
    } else {
      const entries = await readdir(path);
      await Promise.all(
        entries.map((entry) => rm(join(path, entry), { recursive: true, force: true })),
      );
    }
  } catch {
    // Gone already, or never made; anything that's left is for the next clone there to point out.
  }
}

/**
 * A line of git's progress, like `Receiving objects:  45% (450/1000), 1.20 MiB | 600.00 KiB/s` or
 * `remote: Counting objects:  12% (1/8)`; `undefined` for any other line.
 */
export function parseProgress(line: string): CloneProgress | undefined {
  const match = /^(?:remote: )?([A-Z][\w ]*?):\s+(\d{1,3})% /.exec(`${line.trim()} `);
  if (!match) return undefined;
  return { phase: match[1]!, percent: Math.min(100, Number(match[2])) };
}

/**
 * Why a clone failed, from what git wrote to stderr: without its progress and the `fatal:` before
 * its reasons, and in words that say what to do where git's don't help in the app.
 */
export function failure(stderr: string): string {
  const lines: string[] = [];
  let checkoutFailed = false;
  for (const raw of stderr.split(/[\r\n]/)) {
    const line = raw.trim();
    if (!line || parseProgress(line) || line.startsWith("Cloning into ")) continue;
    // What git says to do about a checkout that failed, in a terminal, of a clone that's deleted.
    if (line.endsWith("Clone succeeded, but checkout failed.")) {
      checkoutFailed = true;
      continue;
    }
    if (/^(You can inspect what was checked out|and retry with ')/.test(line)) continue;
    // The remote's own progress, like `remote: Enumerating objects: 5, done.`.
    if (/^remote: ([\w ]+: \d+(, done)?\.?|Total \d+.*)$/.test(line)) continue;
    const reason = line.replace(/^(fatal|error): /, "");
    // The rest of a sentence git wraps over lines, like `Please make sure you have the correct
    // access rights` and `and the repository exists.`, on one.
    const previous = lines.at(-1);
    if (reason === line && /^[a-z]/.test(line) && previous && !/[.!?:]$/.test(previous)) {
      lines[lines.length - 1] = `${previous} ${line}`;
    } else {
      lines.push(reason.charAt(0).toUpperCase() + reason.slice(1));
    }
  }
  const text = lines.join("\n");
  // GIT_TERMINAL_PROMPT=0 (see the runner): git couldn't ask for a user name and password.
  const prompt = /could not read (?:Username|Password) for '([^']+)'/i.exec(text);
  if (prompt) {
    return `Git couldn't sign in to ${host(prompt[1]!)}. Set up a credential helper for it, or clone with an SSH URL.`;
  }
  if (checkoutFailed) {
    const deleted = "The files couldn't be checked out, so the clone was deleted.";
    return text ? `${deleted}\n${text}` : deleted;
  }
  return text || "Git couldn't clone the repository.";
}

/** The host of `url`, or `url` itself if it hasn't one. */
function host(url: string): string {
  try {
    return new URL(url).host || url;
  } catch {
    return url;
  }
}
