import type { Commit } from "@gitto/git/types";
import {
  keepPreviousData,
  useQuery,
  useQueryClient,
  type QueryFunctionContext,
} from "@tanstack/solid-query";
import { createMemo, createSignal } from "solid-js";

import { buildHistory, toCommitRow, type History, type HistoryRow } from "@/git/rows";
import { hasUncommittedChanges, headSha } from "@/git/status";
import { Raw } from "@/lib/raw";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useStatusNow } from "./status";

const NO_ROWS: HistoryRow[] = [];

/** How many commits are loaded to begin with, and how many more each time the end comes near. */
const FIRST_PAGE = 200;
const MORE_PAGE = 1000;
/**
 * How many commits are loaded again, at most, when the history changed: past this, the ones that
 * were scrolled to are dropped, rather than laying all of them out again on every commit.
 */
const MAX_RELOADED = 10_000;

/** The log as far as it's loaded: tagged with its repository, so every row knows where its commit lives. */
export interface Log {
  repositoryId: string;
  /** Newest first. */
  commits: Commit[];
  /** How many were asked for; fewer commits than that means the history ends there. */
  limit: number;
  /** The version the commits came with, when they came in one go: lets a reload skip them if nothing changed. */
  version: string | undefined;
}

/** Whether that's all of the history. */
export function isComplete(log: Log): boolean {
  return log.commits.length < log.limit;
}

// Defined once, so the same log isn't wrapped again. Kept out of the query's store (see `Raw`):
// a long history is thousands of commits.
const selectLog = (log: Log): Raw<Log> => new Raw(log);

export function useLog(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    const queryKey = gitKeys.log(id);
    return {
      queryKey,
      queryFn: async ({ signal, client }: QueryFunctionContext): Promise<Log> => {
        for (let attempt = 1; ; attempt++) {
          // A reload asks for as many commits as were loaded, in one go.
          const previous = client.getQueryData<Log>(queryKey);
          const limit = Math.min(previous?.limit ?? FIRST_PAGE, MAX_RELOADED);
          // oxlint-disable-next-line no-await-in-loop -- only again when the first answer is outdated
          const page = await rpc.git.history.log(
            {
              repositoryId: id,
              limit,
              since: previous?.limit === limit ? previous.version : undefined,
            },
            { signal },
          );
          // Older commits were added meanwhile (see `loadMore`): this would drop them again, so
          // it starts over with them. Not forever, should someone keep scrolling: the next reload
          // brings them back.
          if (client.getQueryData(queryKey) !== previous && attempt < 3) continue;
          // Unchanged: keep the same log, so the history isn't laid out and rendered again. Only
          // ever the answer when there was a previous log to compare with.
          if ("unchanged" in page) return previous!;
          return { repositoryId: id, commits: page.commits, limit, version: page.version };
        }
      },
      select: selectLog,
    };
  });
}

/**
 * History table rows: the uncommitted changes (if any) followed by the log. `selectedId` picks the
 * selected row; it defaults to the first one, once it's known whether there are uncommitted changes.
 * The log starts with its newest commits; `loadMore` adds older ones.
 */
export function useHistory(repositoryId: () => string, selectedId: () => string | undefined) {
  const queryClient = useQueryClient();
  const log = useLog(repositoryId);
  // Not waited for: the log shows as soon as it's in, and the uncommitted changes join it once
  // the status is.
  const status = useStatusNow(repositoryId);

  // The layout only depends on the log, whether there are changes and HEAD; not on the rest of
  // the status, which is refetched whenever a file changes.
  const hasChanges = createMemo(() => hasUncommittedChanges(status().data));
  const head = createMemo(() => {
    const summary = status().data;
    return summary && headSha(summary.head);
  });
  const history = createMemo<History | undefined>((previous) => {
    const loaded = log.data?.value;
    if (!loaded) return undefined;
    const { repositoryId: id, commits } = loaded;
    return buildHistory(
      { repositoryId: id, commits, hasChanges: hasChanges(), head: head() },
      previous,
    );
  });
  const rows = () => history()?.rows ?? NO_ROWS;
  const selected = createMemo(
    () =>
      rows().find((row) => row.id === selectedId()) ??
      // Until the status is in, the first row could still become the uncommitted changes.
      (status().isPending ? undefined : rows()[0]),
  );

  const [loadingMore, setLoadingMore] = createSignal(false);
  // The log that loading more after failed, if it did.
  const [failedAfter, setFailedAfter] = createSignal<Log>();
  /**
   * Adds the next, older commits to the log, unless that's happening already or there are none.
   * After it failed, it isn't tried again by itself (see `loadFailed`).
   */
  async function loadMore() {
    const id = repositoryId();
    const queryKey = gitKeys.log(id);
    const loaded = queryClient.getQueryData<Log>(queryKey);
    if (!loaded || isComplete(loaded) || loadingMore()) return;
    setLoadingMore(true);
    setFailedAfter(undefined);
    try {
      const skip = loaded.commits.length;
      const page = await rpc.git.history.log({ repositoryId: id, limit: MORE_PAGE, skip });
      // Only added to the log they continue: one reloaded meanwhile may start somewhere else.
      if ("unchanged" in page || queryClient.getQueryData(queryKey) !== loaded) return;
      // Nor to one that's to be reloaded later, e.g. as the repository is no longer watched:
      // setting it would make it count as up to date again. One being reloaded right now takes
      // the commits along (see `useLog`).
      const state = queryClient.getQueryState(queryKey);
      if (state?.isInvalidated && state.fetchStatus !== "fetching") return;
      // A commit made meanwhile shifts the rest, so the page can start with ones there already.
      const known = new Set(loaded.commits.map((commit) => commit.sha));
      const added = page.commits.filter((commit) => !known.has(commit.sha));
      queryClient.setQueryData<Log>(queryKey, {
        repositoryId: id,
        commits: [...loaded.commits, ...added],
        // The end of the history is where the page came up short.
        limit: skip + added.length + (page.commits.length < MORE_PAGE ? 1 : 0),
        version: undefined,
      });
    } catch (error) {
      console.error("Couldn't load more of the history", error);
      setFailedAfter(loaded);
    } finally {
      setLoadingMore(false);
    }
  }
  /** Whether the whole history is loaded. */
  const complete = () => {
    const loaded = log.data?.value;
    return !loaded || isComplete(loaded);
  };
  /** How many commits are loaded. */
  const loadedCount = () => log.data?.value.commits.length ?? 0;
  /** Whether loading more of the log on show failed: it's only tried again when asked. */
  const loadFailed = () => {
    const failed = failedAfter();
    return failed !== undefined && failed === log.data?.value;
  };

  return { log, rows, selected, loadMore, loadingMore, loadFailed, complete, loadedCount };
}

/** A single commit, loaded on its own, e.g. for the details of the selected one. */
export function useCommitDetails(repositoryId: () => string, sha: () => string) {
  const queryClient = useQueryClient();
  const query = useQuery(() => {
    const id = repositoryId();
    const commitSha = sha();
    return {
      queryKey: gitKeys.commit(id, commitSha),
      queryFn: ({ signal }: { signal: AbortSignal }) =>
        rpc.git.history.commit({ repositoryId: id, sha: commitSha }, { signal }),
      // The log has its commits in full, so one selected there shows without asking git again.
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
