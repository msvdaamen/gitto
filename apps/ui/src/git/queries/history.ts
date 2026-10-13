import type { Commit } from "@gitto/git/types";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { checkouts } from "@/git/ref-labels";
import { historyGraph, toCommitRow, toHistoryRows, withStashes } from "@/git/rows";
import { hasUncommittedChanges } from "@/git/status";
import { useResetOn } from "@/hooks/reset-on";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useStashes } from "./stash";
import { useHeadSha, useStatus } from "./status";
import { useWorktrees } from "./worktree";

/** How many commits the history loads first: a few screenfuls, so it shows quickly. */
const FIRST_PAGE = 200;

/** How many more it loads at a time as the end of what's loaded comes into view. */
const NEXT_PAGE = 1000;

/** A repository's log, as `useLog` loads it: its newest commits, and those loaded after them. */
interface Log {
  repositoryId: string;
  commits: Commit[];
  /** Whether that's all of the history: the last page loaded was short of its limit. */
  complete: boolean;
}

/**
 * A repository's log: the first `FIRST_PAGE` commits, then `NEXT_PAGE` more each time `more` is
 * run. A refetch, as the repository changes, starts over from the first page: reloading every page
 * loaded so far would take a while once there are a few.
 */
export function useLog(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const query = useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.log(id),
      // Tagged with its repository, so every row knows where its commit lives (see `toHistoryRows`).
      queryFn: async ({ signal }): Promise<Log> => {
        const commits = await rpc.git.history.log(
          { repositoryId: id, skip: 0, limit: FIRST_PAGE },
          { signal },
        );
        return { repositoryId: id, commits, complete: commits.length < FIRST_PAGE };
      },
      // Merged into the previous log, commit by commit, so a refetch that brings nothing new
      // (the refs changed elsewhere, say) doesn't lay out and render the history again.
      reconcile: "sha",
    };
  });

  const more = useMutation(() => ({
    mutationFn: async (id: string) => {
      const key = gitKeys.log(id);
      const log = queryClient.getQueryData<Log>(key);
      if (!log || log.complete) return;
      const updatedAt = queryClient.getQueryState(key)?.dataUpdatedAt;
      const commits = await rpc.git.history.log({
        repositoryId: id,
        skip: log.commits.length,
        limit: NEXT_PAGE,
      });
      // Reloaded meanwhile, so back to the first page, which this one doesn't follow on from.
      if (queryClient.getQueryState(key)?.dataUpdatedAt !== updatedAt) return;
      // Each commit only once: the history may have moved since the commits before were loaded,
      // until the refetch that follows the move.
      const listed = new Set(log.commits.map((commit) => commit.sha));
      queryClient.setQueryData<Log>(key, {
        ...log,
        commits: [...log.commits, ...commits.filter((commit) => !listed.has(commit.sha))],
        complete: commits.length < NEXT_PAGE,
      });
    },
  }));
  // Whether loading more failed, or is still under way, is of the log as it was: not of another
  // repository's, nor of the one reloaded since.
  useResetOn(
    () => [repositoryId(), query.dataUpdatedAt],
    () => more.reset(),
  );

  return { query, more };
}

/**
 * History table rows: the uncommitted changes (if any) followed by the log, with the stashes among
 * it. `selectedId` picks the selected row; it defaults to the first one.
 */
export function useHistory(repositoryId: () => string, selectedId: () => string | undefined) {
  const { query: log, more } = useLog(repositoryId);
  const status = useStatus(repositoryId);
  const stashes = useStashes(repositoryId);
  const worktrees = useWorktrees(repositoryId);

  // The layout only depends on the log, the stashes, whether there are changes and HEAD; not on the
  // rest of the status, which is refetched whenever a file changes. The log, the stashes and the
  // worktrees are reconciled when they're refetched, so they only change when there's something
  // new.
  const hasChanges = createMemo(() => hasUncommittedChanges(status.data));
  const head = useHeadSha(() => status.data);
  // Without the stashes until they've loaded, rather than holding up the history for them (reading
  // `data` before then would suspend it), and if they couldn't be: the Pop button says why.
  const entries = createMemo(() =>
    log.data ? withStashes(log.data.commits, stashes.isSuccess ? stashes.data : []) : [],
  );
  const graph = createMemo(() => historyGraph(entries(), hasChanges(), head()));
  // Where the other worktrees are checked out, for the labels; without them until they've loaded,
  // and if they couldn't be, as with the stashes.
  const others = createMemo(() => checkouts(worktrees.isSuccess ? worktrees.data : []));
  const rows = createMemo(() =>
    log.data
      ? toHistoryRows(log.data.repositoryId, entries(), hasChanges(), graph(), others())
      : [],
  );
  const selected = createMemo(() => rows().find((row) => row.id === selectedId()) ?? rows()[0]);

  // Whether there's more of the log to load, once it's loaded.
  const hasMore = () => log.data?.complete === false;
  /** Loads more of the log, unless it's loading, or reloading, which would leave that unused. */
  const loadMore = () => {
    if (hasMore() && !log.isFetching && !more.isPending) more.mutate(repositoryId());
  };

  return { log, more, hasMore, loadMore, rows, selected };
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
