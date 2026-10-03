import type { Commit } from "@gitto/git/types";
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { historyGraph, toCommitRow, toHistoryRows, withStashes } from "@/git/rows";
import { hasUncommittedChanges } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useStashes } from "./stash";
import { useHeadSha, useStatus } from "./status";

/** A repository's log, as `useLog` loads it. */
interface Log {
  repositoryId: string;
  commits: Commit[];
}

export function useLog(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.log(id),
      // Tagged with its repository, so every row knows where its commit lives (see `toHistoryRows`).
      queryFn: async ({ signal }): Promise<Log> => ({
        repositoryId: id,
        commits: await rpc.git.history.log({ repositoryId: id }, { signal }),
      }),
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

/**
 * A single commit, e.g. for the details of the selected one. Taken from the log when it's in it,
 * as the selected one is: the log has all of a commit, so git isn't asked for it again. Loaded on
 * its own otherwise.
 */
export function useCommitDetails(repositoryId: () => string, sha: () => string) {
  const queryClient = useQueryClient();
  const query = useQuery(() => {
    const id = repositoryId();
    const commitSha = sha();
    return {
      queryKey: gitKeys.commit(id, commitSha),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.history.commit({ repositoryId: id, sha: commitSha }, { signal }),
      initialData: () =>
        queryClient
          .getQueryData<Log>(gitKeys.log(id))
          ?.commits.find((commit) => commit.sha === commitSha),
      staleTime: Infinity,
      // Keep showing the previous selection while the next one loads, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });

  const commit = createMemo(() => query.data && toCommitRow(repositoryId(), query.data));

  return { query, commit };
}
