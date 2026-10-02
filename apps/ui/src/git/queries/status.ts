import type { StatusSummary, Uncommitted, WorkingTreeFiles } from "@gitto/git/types";
import { useQuery, type QueryFunctionContext } from "@tanstack/solid-query";

import { useQueryResult } from "@/lib/query";
import { Raw } from "@/lib/raw";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/**
 * The uncommitted changes come in one query (so the working tree is only walked once), but most of
 * the UI only shows the summary. Each `useQuery` keeps what it selects in a store of its own, so
 * the file lists, which can be huge, are only selected where they're shown, and kept out of the
 * store there (see `Raw`).
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
const selectChanges = (data: Uncommitted): Raw<WorkingTreeFiles> => new Raw(data.changes);

/** Where HEAD is, and how many files changed. */
export function useStatus(repositoryId: () => string) {
  return useQuery(() => uncommittedQuery(repositoryId(), selectSummary));
}

/**
 * The same, as far as it's loaded: its `data` is `undefined` until then, instead of making
 * everything around it wait. The status walks the working tree, which takes a while in a big one,
 * and the history and the branches can show meanwhile.
 */
export function useStatusNow(repositoryId: () => string) {
  return useQueryResult(() => uncommittedQuery(repositoryId(), selectSummary));
}

/**
 * The staged and unstaged changes. New lists whenever they changed; the rows showing them are
 * rendered by position, so only what's different in the ones in view is drawn again.
 */
export function useUncommittedFiles(repositoryId: () => string) {
  return useQuery(() => ({
    ...uncommittedQuery(repositoryId(), selectChanges),
    // Nothing that shows the lists cares when a refetch starts and ends.
    notifyOnChangeProps: ["data", "error"],
  }));
}
