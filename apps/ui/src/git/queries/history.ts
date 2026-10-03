import type { Log } from "@gitto/git/types";
import { keepPreviousData, useQuery, type QueryFunctionContext } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { historyGraph, toCommitRow, toHistoryRows, withStashes } from "@/git/rows";
import { hasUncommittedChanges } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useStashes } from "./stash";
import { useHeadSha, useStatus } from "./status";

/** A page of the log, tagged with its repository, so every row knows where its commit lives. */
type HistoryLog = Log & { repositoryId: string };

export function useLog(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    const queryKey = gitKeys.log(id);
    return {
      queryKey,
      queryFn: async ({ signal, client }: QueryFunctionContext): Promise<HistoryLog> => {
        const previous = client.getQueryData<HistoryLog>(queryKey);
        const result = await rpc.git.history.log(
          { repositoryId: id, since: previous?.version },
          { signal },
        );
        // Unchanged: keep the same data, without the log being read again. Only ever the answer
        // when there was a previous log to compare with.
        return "unchanged" in result ? previous! : { repositoryId: id, ...result };
      },
      // Merged into the previous log, commit by commit, so a refetch that brings nothing new
      // (the refs changed elsewhere, say) doesn't lay out and render the history again.
      reconcile: "sha",
    };
  });
}

/**
 * History table rows: the uncommitted changes (if any) followed by the log, with the stashes among
 * it. `selectedId` picks the selected row; it defaults to the first one.
 */
export function useHistory(repositoryId: () => string, selectedId: () => string | undefined) {
  const log = useLog(repositoryId);
  const status = useStatus(repositoryId);
  const stashes = useStashes(repositoryId);

  // The layout only depends on the log, the stashes, whether there are changes and HEAD; not on the
  // rest of the status, which is refetched whenever a file changes. The log and the stashes are
  // reconciled when they're refetched, so they only change when there's something new.
  const hasChanges = createMemo(() => hasUncommittedChanges(status.data));
  const head = useHeadSha(status);
  // Without the stashes until they've loaded, rather than holding up the history for them (reading
  // `data` before then would suspend it), and if they couldn't be: the Pop button says why.
  const entries = createMemo(() =>
    log.data ? withStashes(log.data.commits, stashes.isSuccess ? stashes.data : []) : [],
  );
  const graph = createMemo(() => historyGraph(entries(), hasChanges(), head()));
  const rows = createMemo(() =>
    log.data ? toHistoryRows(log.data.repositoryId, entries(), hasChanges(), graph()) : [],
  );
  const selected = createMemo(() => rows().find((row) => row.id === selectedId()) ?? rows()[0]);

  return { log, rows, selected };
}

/** A single commit, loaded on its own, e.g. for the details of the selected one. */
export function useCommitDetails(repositoryId: () => string, sha: () => string) {
  const query = useQuery(() => {
    const id = repositoryId();
    const commitSha = sha();
    return {
      queryKey: gitKeys.commit(id, commitSha),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.history.commit({ repositoryId: id, sha: commitSha }, { signal }),
      staleTime: Infinity,
      // Keep showing the previous selection while the next one loads, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });

  const commit = createMemo(() => query.data && toCommitRow(repositoryId(), query.data));

  return { query, commit };
}
