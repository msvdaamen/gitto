import type { ChangedFile } from "@gitto/git/types";

import type { DiffSource } from "./diff-source";

/** Lines added and removed. */
export interface LineCounts {
  additions: number;
  deletions: number;
}

/** Lines added and deleted across `files`; binary files don't count. */
export function lineTotals(files: ChangedFile[]): LineCounts {
  return files.reduce(
    (sum, file) => ({
      additions: sum.additions + (file.additions ?? 0),
      deletions: sum.deletions + (file.deletions ?? 0),
    }),
    { additions: 0, deletions: 0 },
  );
}

/** Paths to stage or unstage for files; renames need their old path too, for the deletion side. */
export function stagingPaths(files: ChangedFile[]): string[] {
  return files.flatMap((file) => (file.origPath ? [file.path, file.origPath] : [file.path]));
}

/** A file's line counts, as its list has them; `undefined` if they weren't counted. */
export function countsOf(file: ChangedFile): LineCounts | undefined {
  return file.additions === null
    ? undefined
    : { additions: file.additions, deletions: file.deletions ?? 0 };
}

/** How many lines changed in all: added and removed. */
export function totalLines(counts: LineCounts): number {
  return counts.additions + counts.deletions;
}

/**
 * Whether `file`'s list says it's binary: it has no line counts, though its list does
 * (`uncounted` says whether it counted any).
 */
export function isKnownBinary(file: ChangedFile, uncounted: boolean): boolean {
  return file.additions === null && !uncounted && file.status !== "untracked";
}

/**
 * Whether `file` is a change of this repository's own files: not a conflict, which is resolved
 * instead, nor a folder git lists as untracked, which is a repository of its own.
 */
export function isOwnChange(file: ChangedFile): boolean {
  return file.status !== "conflicted" && !isNestedRepository(file);
}

/**
 * Whether `file` has changes to show as a patch, as far as its list can tell: not for one that
 * isn't this repository's own (see `isOwnChange`), nor one that's binary or without changed lines.
 */
export function hasPatch(file: ChangedFile, uncounted: boolean): boolean {
  if (!isOwnChange(file)) return false;
  if (isKnownBinary(file, uncounted)) return false;
  const counts = countsOf(file);
  return !counts || totalLines(counts) > 0;
}

/** Whether `file` is a repository inside this one: git lists one as an untracked folder. */
export function isNestedRepository(file: ChangedFile): boolean {
  return file.status === "untracked" && file.path.endsWith("/");
}

/** Why a file without binary contents has no changed lines. */
export function unchangedReason(file: ChangedFile): string {
  if (file.origPath) return `Renamed from ${file.origPath}, with the same contents.`;
  if (file.status === "added" || file.status === "untracked") return "An empty file was added.";
  if (file.status === "deleted") return "An empty file was deleted.";
  return "Only the file's mode changed.";
}

/** Which side of the uncommitted changes the file's are, to say next to its name. */
export function sourceLabel(source: DiffSource): string | undefined {
  if (source.kind === "unstaged") return "Unstaged";
  if (source.kind === "staged") return "Staged";
  return undefined;
}
