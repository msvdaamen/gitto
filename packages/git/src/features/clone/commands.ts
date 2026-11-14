import { readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { isAbsolute, resolve } from "node:path";

import { CloneFailedError, GitError } from "../../core/errors";
import { runGit } from "../../core/runner";
import type { CloneProgress } from "./schema";

/**
 * Clones the repository at `url` into `folder`, which must be empty or not there yet: its full
 * path, or one starting with `~` for the home folder. Yields how far it's got as git reports it,
 * only the latest when several come at once, and returns the path it cloned into. Stopped by
 * `signal`, or when Gitto exits, git deletes what it cloned so far, as it does when it fails.
 */
export async function* cloneRepository(
  url: string,
  folder: string,
  signal?: AbortSignal,
): AsyncGenerator<CloneProgress, string> {
  const path = clonePath(folder);
  await checkFolder(path);

  // Also stopped if whatever reads this stops before the clone is done.
  const stop = new AbortController();
  let latest: CloneProgress | undefined;
  let finished = false;
  let wake: (() => void) | undefined;
  // Git rewrites a progress line with `\r` as it goes, and ends it with `\n`; the rest of a line
  // comes with the next chunk.
  let partial = "";
  const onStderr = (text: string) => {
    const lines = (partial + text).split(/[\r\n]/);
    partial = lines.pop()!;
    for (const line of lines) latest = parseProgress(line) ?? latest;
    if (latest) wake?.();
  };
  const clone = runGit(homedir(), ["clone", "--progress", "--", url.trim(), path], {
    signal: signal ? AbortSignal.any([signal, stop.signal]) : stop.signal,
    onStderr,
    stopOnExit: true,
  }).finally(() => {
    finished = true;
    wake?.();
  });
  // Awaited once the progress is read, below.
  clone.catch(() => undefined);

  try {
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
  } finally {
    if (!finished) stop.abort();
  }

  try {
    await clone;
  } catch (error) {
    throw error instanceof GitError ? new CloneFailedError(failure(error.stderr)) : error;
  }
  return path;
}

/** `folder` as a full path, with `~` for the home folder; rejects one that isn't a full path. */
export function clonePath(folder: string): string {
  const trimmed = folder.trim();
  const expanded = /^~(?=$|[\\/])/.test(trimmed) ? homedir() + trimmed.slice(1) : trimmed;
  if (!isAbsolute(expanded)) {
    throw new CloneFailedError(
      `${trimmed} isn't a full path. Choose the folder to clone into, or type its full path.`,
    );
  }
  return resolve(expanded);
}

/**
 * Rejects if `path` is a folder with something in it, which git won't clone into, saying so in
 * words that help in the app. Anything else in the way, like a file there, is for git to explain.
 */
async function checkFolder(path: string): Promise<void> {
  const entries = await readdir(path).catch(() => []);
  if (entries.length > 0) {
    throw new CloneFailedError(
      `${path} isn't empty. Clone into an empty folder, or one that isn't there yet.`,
    );
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
  for (const raw of stderr.split(/[\r\n]/)) {
    const line = raw.trim();
    if (!line || parseProgress(line) || line.startsWith("Cloning into ")) continue;
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
