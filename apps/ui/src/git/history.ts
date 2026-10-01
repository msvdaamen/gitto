import { keepPreviousData, useQuery } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { hasUncommittedChanges, historyGraph, toCommitRow, toCommitRows } from "./rows";
import { useStatus } from "./status";

export function useLog(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.log(id),
      // Tagged with its repository, so every row knows where its commit lives (see `toCommitRows`).
      queryFn: async ({ signal }) => ({
        repositoryId: id,
        commits: await rpc.git.history.log({ repositoryId: id }, { signal }),
      }),
    };
  });
}

/**
 * History table rows: the uncommitted changes (if any) followed by the log. `selectedId` picks the
 * selected row; it defaults to the first one.
 */
export function useHistory(repositoryId: () => string, selectedId: () => string | undefined) {
  const log = useLog(repositoryId);
  const status = useStatus(repositoryId);

  // The layout only depends on the log, whether there are changes and HEAD; not on the rest of
  // the status, which is refetched whenever a file changes.
  const hasChanges = createMemo(() => hasUncommittedChanges(status.data));
  const head = createMemo(() => {
    const current = status.data?.head;
    return current && current.kind !== "unborn" ? current.sha : undefined;
  });
  const graph = createMemo(() =>
    log.data ? historyGraph(log.data.commits, hasChanges(), head()) : [],
  );
  const rows = createMemo(() =>
    log.data ? toCommitRows(log.data.repositoryId, log.data.commits, hasChanges(), graph()) : [],
  );
  const selected = createMemo(() => rows().find((row) => row.id === selectedId()) ?? rows()[0]);

  return { log, rows, selected };
}

/** A single commit, loaded on its own, e.g. for the details of the selected one. */
export function useCommit(repositoryId: () => string, sha: () => string) {
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
