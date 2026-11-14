// Where to suggest cloning a repository to, from its URL, and how far a clone has got.

import type { CloneProgress } from "@gitto/git/types";

/**
 * The name of the repository at `url`, as `git clone` names its folder: the last part of its
 * path, without `.git`. E.g. `gitto` for `https://github.com/owner/gitto.git`,
 * `git@github.com:owner/gitto.git` or `/srv/git/gitto/`; empty if there's none yet.
 */
export function repositoryName(url: string): string {
  const path = url
    .trim()
    // Without a query or fragment, and the slashes and `.git` at the end.
    .replace(/[?#].*$/, "")
    .replace(/[\\/]+$/, "")
    .replace(/\.git$/i, "")
    .replace(/[\\/]+$/, "");
  // After the last slash, or the colon of `host:path`, or of `scheme://host` when there's no path.
  const name = path.split(/[\\/:]/).pop() ?? "";
  return /^[.]*$/.test(name) || /^[\w.-]+@/.test(name) ? "" : name;
}

/** The path separator `path` uses: `\` on Windows, `/` elsewhere. */
function separatorOf(path: string): string {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

/** The folder `path` is in; `undefined` for a root, or a path without a folder. */
export function parentFolder(path: string): string | undefined {
  const separator = separatorOf(path);
  const trimmed = path.length > 1 && path.endsWith(separator) ? path.slice(0, -1) : path;
  const at = trimmed.lastIndexOf(separator);
  if (at < 0) return undefined;
  // `/home` is in `/`, and `C:\code` in `C:\`.
  const parent = trimmed.slice(0, at);
  return parent === "" || /^[A-Za-z]:$/.test(parent) ? parent + separator : parent;
}

/** `name` in the folder `parent`, with the separator `parent` uses. */
export function inFolder(parent: string, name: string): string {
  const separator = separatorOf(parent);
  return parent.endsWith(separator) ? parent + name : `${parent}${separator}${name}`;
}

/**
 * The part of a whole clone each phase git reports is, from and to which percent: most of the time
 * goes to receiving the objects.
 */
const PHASES: Record<string, readonly [number, number]> = {
  "Counting objects": [0, 4],
  "Compressing objects": [4, 8],
  "Receiving objects": [8, 80],
  "Resolving deltas": [80, 92],
  "Updating files": [92, 100],
  // Git LFS, as the files are checked out.
  "Filtering content": [92, 100],
};

/**
 * How far a clone has got, as of `reported` (by git, for its phase), after `last`: the phase, and
 * how much of the whole clone is done, which only goes up. A phase git reports that's not known
 * here keeps the percent where it was.
 */
export function overallProgress(
  last: CloneProgress | null,
  reported: CloneProgress,
): CloneProgress {
  const before = last?.percent ?? 0;
  const span = PHASES[reported.phase];
  if (!span) return { phase: reported.phase, percent: before };
  const [from, to] = span;
  const percent = Math.round(from + ((to - from) * reported.percent) / 100);
  return { phase: reported.phase, percent: Math.max(before, percent) };
}
