import type { ChangedFile } from "@gitto/git/types";

/**
 * Where the changes in a file on show are from: a commit or a stash, which never change, or one
 * side of the uncommitted changes, which change on disk: what isn't staged (the working tree
 * compared to the index), or what is (the index compared to HEAD).
 */
export type DiffSource = { kind: "commit" | "stash"; sha: string } | UncommittedSource;

export type UncommittedSource = { kind: "unstaged" | "staged" };

export function isUncommitted(source: DiffSource): source is UncommittedSource {
  return source.kind === "unstaged" || source.kind === "staged";
}

/**
 * Names a file in `source`, to tell whether it's the one on show: the same path can be both in the
 * staged and the unstaged changes.
 */
export function diffFileKey(source: DiffSource, path: string): string {
  return `${source.kind}:${isUncommitted(source) ? "" : source.sha}:${path}`;
}

/** Whether `a` and `b` are the same commit, stash, or side of the uncommitted changes. */
export function isSameSource(a: DiffSource, b: DiffSource): boolean {
  return diffFileKey(a, "") === diffFileKey(b, "");
}

/**
 * Shows the changes in a file of one of the lists in the details, and says which one is on show.
 * The details of the selected row pass it to each list, with the source of its files.
 */
export interface FileOpener {
  open: (source: DiffSource, file: ChangedFile) => void;
  /** Loads a file's changes ahead, as it's likely to be opened; see `prefetchFileDiff`. */
  prefetch: (source: DiffSource, file: ChangedFile, uncounted: boolean) => void;
  /** The file whose changes are on show, if any. */
  shown: { source: DiffSource; path: string } | undefined;
  /**
   * What changing the working tree waits for, like discarding changes: the edits to the file on
   * show saved. Resolves to whether to go on.
   */
  beforeChange: () => Promise<boolean>;
}

/** The path of the file on show, if it's one of `source`'s, for its list to mark it. */
export function shownPathIn(opener: FileOpener | undefined, source: DiffSource) {
  const shown = opener?.shown;
  return shown && isSameSource(shown.source, source) ? shown.path : undefined;
}
