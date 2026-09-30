import type { ChangedFile } from "@gitto/git/types";
import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";
import type { Commit } from "@/types/git";

import { gitKeys } from "./keys";

/** Files changed by a commit, compared to its first parent. */
export function useCommitFiles(commit: () => Commit) {
  const query = useQuery(() => {
    const { repositoryId, id } = commit();
    return {
      queryKey: gitKeys.commitFiles(repositoryId, id),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.diff.commitFiles({ repositoryId, sha: id }, { signal }),
      staleTime: Infinity,
      // Keep showing the previous selection's files while the next ones load, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });

  const files = createMemo(() => query.data ?? []);
  const totals = createMemo(() => lineTotals(files()));

  return { query, files, totals };
}

/** The uncommitted changes, split into what's staged for the next commit and what isn't. */
export function useWorkingTreeChanges(repositoryId: () => string) {
  const query = useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.workingTreeFiles(id),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.diff.workingTreeFiles({ repositoryId: id }, { signal }),
      staleTime: 0,
    };
  });

  // A conflict shows up on both sides, but it's resolved (and so staged) by staging it.
  const staged = createMemo(() =>
    (query.data?.staged ?? []).filter((file) => file.status !== "conflicted"),
  );
  const unstaged = createMemo(() => query.data?.unstaged ?? []);

  return { query, staged, unstaged };
}

function lineTotals(files: ChangedFile[]) {
  return files.reduce(
    (sum, file) => ({
      additions: sum.additions + (file.additions ?? 0),
      deletions: sum.deletions + (file.deletions ?? 0),
    }),
    { additions: 0, deletions: 0 },
  );
}
