import type { StatusSummary, Uncommitted, WorkingTreeFiles } from "@gitto/git/types";
import { useQuery, type QueryFunctionContext } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { headSha } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/**
 * The uncommitted changes come in one query (so the working tree is only walked once), but most of
 * the UI only shows the summary. Each `useQuery` keeps its own copy of what it selects, so the file
 * lists, which can be huge, are only selected (and kept up to date) where they're shown.
 */
function uncommittedQuery<T>(id: string, select: (data: Uncommitted) => T) {
  const queryKey = gitKeys.status(id);
  return {
    queryKey,
    queryFn: async ({ signal, client }: QueryFunctionContext) => {
      const previous = client.getQueryData<Uncommitted>(queryKey);
      const result = await rpc.git.status.get(
        { repositoryId: id, since: previous?.version },
        { signal },
      );
      // Unchanged: keep the same data, so nothing selected from it changes or renders again. Only
      // ever the answer when there was a previous status to compare with.
      return "unchanged" in result ? previous! : result;
    },
    select,
  };
}

// Defined once, so a refetch that returns the same data doesn't select it again.
const selectSummary = ({ head, upstream, ahead, behind, counts }: Uncommitted): StatusSummary => ({
  head,
  upstream,
  ahead,
  behind,
  counts,
});
const selectChanges = (data: Uncommitted): WorkingTreeFiles => data.changes;

/** Where HEAD is, and how many files changed. */
export function useStatus(repositoryId: () => string) {
  return useQuery(() => uncommittedQuery(repositoryId(), selectSummary));
}

/**
 * The commit HEAD points at, from a `useStatus` query; `undefined` before the first commit, or until
 * the status loads. Only changes when HEAD moves, not with every status refetch.
 */
export function useHeadSha(status: { data: StatusSummary | undefined }) {
  return createMemo(() => status.data && headSha(status.data.head));
}

/** The staged and unstaged changes, with their line counts. */
export function useUncommittedFiles(repositoryId: () => string) {
  return useQuery(() => ({
    ...uncommittedQuery(repositoryId(), selectChanges),
    // Merged into the previous lists, file by file, so a refetch only updates what changed:
    // otherwise every file is a new object and its row is rendered again.
    reconcile: "path",
    // Merging is slow for thousands of files, so it's only done when they changed, not also when a
    // refetch starts and ends.
    notifyOnChangeProps: ["data", "error"],
  }));
}
