import type { Commit } from "@gitto/git/types";
import {
  keepPreviousData,
  useInfiniteQuery,
  useQuery,
  useQueryClient,
  type InfiniteData,
} from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { checkouts } from "@/git/ref-labels";
import { historyGraph, toCommitRow, toHistoryRows, withStashes } from "@/git/rows";
import { hasUncommittedChanges } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useStashes } from "./stash";
import { useHeadSha, useStatus } from "./status";
import { useWorktrees } from "./worktree";

/** How many commits the history loads first: a few screenfuls, so it shows quickly. */
const FIRST_PAGE = 200;

/** How many more it loads at a time as the end of what's loaded comes into view. */
const NEXT_PAGE = 1000;

/** A page of a repository's log, as `useLog` loads it. */
interface LogPage {
  repositoryId: string;
  commits: Commit[];
}

/** Where a page of the log starts, and how long it is. */
interface LogPageParam {
  skip: number;
  limit: number;
}

/**
 * A repository's log, a page at a time: the first `FIRST_PAGE` commits, then `NEXT_PAGE` more each
 * time `fetchNextPage` is called. A refetch reloads every page loaded so far.
 */
export function useLog(repositoryId: () => string) {
  return useInfiniteQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.log(id),
      // Tagged with its repository, so every row knows where its commit lives (see `toHistoryRows`).
      queryFn: async ({ pageParam, signal }): Promise<LogPage> => ({
        repositoryId: id,
        commits: await rpc.git.history.log({ repositoryId: id, ...pageParam }, { signal }),
      }),
      initialPageParam: { skip: 0, limit: FIRST_PAGE } as LogPageParam,
      // A page short of its limit is the end of the history. The next one is worked out from the
      // pages before it on a refetch too, so they still follow on from each other when commits
      // were added at the top meanwhile.
      getNextPageParam: (last, _pages, param): LogPageParam | undefined =>
        last.commits.length < param.limit
          ? undefined
          : { skip: param.skip + last.commits.length, limit: NEXT_PAGE },
      // Merged into the previous log, commit by commit, so a refetch that brings nothing new
      // (the refs changed elsewhere, say) doesn't lay out and render the history again.
      reconcile: "sha",
    };
  });
}

/**
 * The commits of every page of `log` loaded so far, newest first. Each only once: a page loaded
 * just as commits were added at the top starts with some the page before has too, until the
 * refetch that follows reloads them all.
 */
function logCommits(log: InfiniteData<LogPage>): Commit[] {
  const listed = new Set<string>();
  return log.pages.flatMap((page) =>
    page.commits.filter((commit) => {
      if (listed.has(commit.sha)) return false;
      listed.add(commit.sha);
      return true;
    }),
  );
}

/**
 * History table rows: the uncommitted changes (if any) followed by the log, with the stashes among
 * it. `selectedId` picks the selected row; it defaults to the first one.
 */
export function useHistory(repositoryId: () => string, selectedId: () => string | undefined) {
  const log = useLog(repositoryId);
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
    log.data ? withStashes(logCommits(log.data), stashes.isSuccess ? stashes.data : []) : [],
  );
  const graph = createMemo(() => historyGraph(entries(), hasChanges(), head()));
  // Where the other worktrees are checked out, for the labels; without them until they've loaded,
  // and if they couldn't be, as with the stashes.
  const others = createMemo(() => checkouts(worktrees.isSuccess ? worktrees.data : []));
  const rows = createMemo(() =>
    log.data
      ? toHistoryRows(log.data.pages[0]!.repositoryId, entries(), hasChanges(), graph(), others())
      : [],
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
          .getQueryData<InfiniteData<LogPage>>(gitKeys.log(id))
          ?.pages.flatMap((page) => page.commits)
          .find((commit) => commit.sha === commitSha),
      staleTime: Infinity,
      // Keep showing the previous selection while the next one loads, instead of suspending.
      placeholderData: keepPreviousData,
    };
  });

  const commit = createMemo(() => query.data && toCommitRow(repositoryId(), query.data));

  return { query, commit };
}
