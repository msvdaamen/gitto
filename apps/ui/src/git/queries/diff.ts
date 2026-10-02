import type { ChangedFile, FileChange, LineCounts } from "@gitto/git/types";
import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { createEffect, createMemo, createSignal, onCleanup, untrack } from "solid-js";

import { lineTotals, stagingPaths } from "@/git/changes";
import { useQueryResult } from "@/lib/query";
import { Raw } from "@/lib/raw";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useUncommittedFiles } from "./status";

// Defined once, so the same files aren't wrapped again. A commit can change thousands of them.
const selectFiles = (files: ChangedFile[]): Raw<ChangedFile[]> => new Raw(files);

/** Files changed by a commit, compared to its first parent. */
export function useCommitFiles(repositoryId: () => string, sha: () => string) {
  const query = useQuery(() => {
    const id = repositoryId();
    const commitSha = sha();
    return {
      queryKey: gitKeys.commitFiles(id, commitSha),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.diff.commitFiles({ repositoryId: id, sha: commitSha }, { signal }),
      select: selectFiles,
      staleTime: Infinity,
      // Keep showing the previous selection's files while the next ones load, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });

  const files = createMemo(() => query.data?.value ?? []);
  const totals = createMemo(() => lineTotals(files()));

  return { query, files, totals };
}

/**
 * The uncommitted changes, split into what's staged for the next commit and what isn't. They come
 * with the status, so this shares its query.
 */
export function useWorkingTreeChanges(repositoryId: () => string) {
  const query = useUncommittedFiles(repositoryId);

  // A conflict shows up on both sides, but it's resolved (and so staged) by staging it.
  const staged = createMemo(() =>
    (query.data?.value.staged ?? []).filter((file) => file.status !== "conflicted"),
  );
  const unstaged = createMemo(() => query.data?.value.unstaged ?? []);

  return { query, staged, unstaged };
}

/**
 * How long the files on show have to stay the same before their lines are counted: scrolling
 * changes them every frame.
 */
const SETTLE_MS = 100;
/** Past this many, the counts kept for files that scrolled out of view are dropped. */
const MAX_KEPT = 5000;

/**
 * Line counts of the uncommitted changes on one side of the index, for the `files` on show: git
 * diffs every file it counts, so it's only asked about those. Returns the counts of a file, if
 * they're in; they stay known for files that were shown before, and are counted again when those
 * come back in view or the working tree changes. An untracked file has none.
 */
export function useLineCounts(
  repositoryId: () => string,
  side: "staged" | "unstaged",
  files: () => FileChange[],
) {
  // Untracked files have nothing to compare with.
  const counted = createMemo(() => files().filter((file) => file.status !== "untracked"));
  const [asked, setAsked] = createSignal<FileChange[]>([]);
  createEffect(() => {
    const next = counted();
    // The first ones right away.
    if (untrack(asked).length === 0) {
      setAsked(next);
      return;
    }
    const timer = setTimeout(() => setAsked(next), SETTLE_MS);
    onCleanup(() => clearTimeout(timer));
  });

  // Not waited for: the files show without their counts until those are in.
  const query = useQueryResult(() => {
    const id = repositoryId();
    const askedFiles = asked().map(({ path, origPath }) => ({ path, origPath }));
    return {
      queryKey: gitKeys.lineCounts(id, side, stagingPaths(askedFiles)),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.diff.lineCounts(
          { repositoryId: id, staged: side === "staged", files: askedFiles },
          { signal },
        ),
      enabled: askedFiles.length > 0,
      placeholderData: keepPreviousData,
      // One of these for every place the list was scrolled to.
      gcTime: 30_000,
    };
  });

  const counts = createMemo<{ id: string; byPath: Map<string, LineCounts> }>((previous) => {
    const id = repositoryId();
    const keep = previous?.id === id && previous.byPath.size < MAX_KEPT;
    const byPath = keep ? previous.byPath : new Map<string, LineCounts>();
    const { data: answer, isPlaceholderData } = query();
    if (answer && !isPlaceholderData) {
      // A file that's no longer in the answer has no changes on this side anymore.
      for (const file of untrack(asked)) byPath.delete(file.path);
      for (const count of answer) byPath.set(count.path, count);
    }
    // A new object, so what reads the counts looks them up again.
    return { id, byPath };
  });

  // Untracked now, the counts it had from before don't apply.
  return (file: FileChange): LineCounts | undefined =>
    file.status === "untracked" ? undefined : counts().byPath.get(file.path);
}
