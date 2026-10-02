import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { lineTotals } from "@/git/changes";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useUncommittedFiles } from "./status";

/** Files changed by a commit, compared to its first parent. */
export function useCommitFiles(repositoryId: () => string, sha: () => string) {
  const query = useQuery(() => {
    const id = repositoryId();
    const commitSha = sha();
    return {
      queryKey: gitKeys.commitFiles(id, commitSha),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.diff.commitFiles({ repositoryId: id, sha: commitSha }, { signal }),
      staleTime: Infinity,
      // Keep showing the previous selection's files while the next ones load, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });

  const files = createMemo(() => query.data ?? []);
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
    (query.data?.staged ?? []).filter((file) => file.status !== "conflicted"),
  );
  const unstaged = createMemo(() => query.data?.unstaged ?? []);

  return { query, staged, unstaged };
}
