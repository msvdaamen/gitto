import type { StatusSummary, Uncommitted, WorkingTreeFiles } from "@gitto/git/types";
import { useQuery, type QueryFunctionContext } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { headSha } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { Opaque } from "./opaque";

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
const selectChanges = (data: Uncommitted) => new Opaque<WorkingTreeFiles>(data.changes);

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

/**
 * The staged and unstaged changes, with their line counts. A refetch that brings changes replaces
 * the lists, rather than being merged into them file by file: there can be tens of thousands of
 * files (see `Opaque`), and their rows are rendered by position (see `VirtualRows`), so only the
 * ones in view are updated either way.
 */
export function useUncommittedFiles(repositoryId: () => string) {
  return useQuery(() => uncommittedQuery(repositoryId(), selectChanges));
}
