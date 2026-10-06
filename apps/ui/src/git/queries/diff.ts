import type { ChangedFile, WorkingTreeFiles } from "@gitto/git/types";
import {
  keepPreviousData,
  queryOptions,
  useQuery,
  type QueryClient,
  type QueryKey,
} from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { lineTotals } from "@/git/changes";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { Opaque } from "./opaque";
import { useUncommittedFiles } from "./status";
import { useUnsuspendedData } from "./unsuspended";

/** Loads the files a commit or stash changed. */
export type FilesFetcher = (
  input: { repositoryId: string; sha: string },
  options: { signal: AbortSignal },
) => Promise<ChangedFile[]>;

/**
 * Loads the files the commit or stash `sha` changed, which never change, with `fetch` under
 * `queryKey`. While another one's load, the previous ones stay on show rather than suspending;
 * they come with the SHA they're of.
 */
export function changedFilesQuery(
  repositoryId: string,
  sha: string,
  queryKey: (repositoryId: string, sha: string) => QueryKey,
  fetch: FilesFetcher,
  enabled = true,
) {
  return queryOptions({
    queryKey: queryKey(repositoryId, sha),
    queryFn: async ({ signal }) => ({
      sha,
      // A commit can change tens of thousands of files.
      files: new Opaque(await fetch({ repositoryId, sha }, { signal })),
    }),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
    enabled,
  });
}

/**
 * The files the commit or stash `sha` changed (see `changedFilesQuery`); `shownSha` says whose are
 * on show, so what's shown with them can match.
 */
export function useChangedFiles(
  repositoryId: () => string,
  sha: () => string,
  queryKey: (repositoryId: string, sha: string) => QueryKey,
  fetch: FilesFetcher,
) {
  const query = useQuery(() => changedFilesQuery(repositoryId(), sha(), queryKey, fetch));

  const files = createMemo(() => query.data?.files.value ?? []);
  const totals = createMemo(() => lineTotals(files()));
  /** The SHA whose files are on show: the previous one's, while `sha`'s load. */
  const shownSha = () => query.data?.sha;

  return { query, files, totals, shownSha };
}

/** Loads the files a commit changed, for `changedFilesQuery`. */
export const fetchCommitFiles: FilesFetcher = (input, options) =>
  rpc.git.diff.commitFiles(input, options);

/** Files changed by a commit, compared to its first parent. */
export function useCommitFiles(repositoryId: () => string, sha: () => string) {
  return useChangedFiles(repositoryId, sha, gitKeys.commitFiles, fetchCommitFiles);
}

/** A file's contents by their object name, e.g. to show more of it around a patch's changes. */
export function fetchBlob(client: QueryClient, repositoryId: string, oid: string): Promise<string> {
  return client.fetchQuery({
    queryKey: gitKeys.blob(repositoryId, oid),
    queryFn: ({ signal }) => rpc.git.diff.blob({ repositoryId, oid }, { signal }),
    staleTime: Infinity,
  });
}

/**
 * The uncommitted changes, split into what's staged for the next commit and what isn't. They come
 * with the status, so this shares its query.
 */
export function useWorkingTreeChanges(repositoryId: () => string) {
  const query = useUncommittedFiles(repositoryId);
  const { staged, unstaged, uncounted } = useWorkingTreeLists(query);
  return { query, staged, unstaged, uncounted };
}

/**
 * The lists in the uncommitted changes `query` loads (see `useUncommittedFiles`): what's staged
 * for the next commit and what isn't, and whether their lines were counted. Read without Suspense:
 * they're refetched while they're on show (see `useUnsuspendedData`).
 */
export function useWorkingTreeLists(query: { data: Opaque<WorkingTreeFiles> | undefined }) {
  const changes = useUnsuspendedData(query);
  const staged = createMemo(() => stagedFiles(changes()?.value.staged ?? []));
  const unstaged = createMemo(() => changes()?.value.unstaged ?? []);
  /** Whether there are too many files for their lines to have been counted. */
  const uncounted = () => changes()?.value.uncounted ?? false;
  /** Whether the changes are loaded: `undefined` until they are. */
  const loaded = () => changes() !== undefined;
  return { staged, unstaged, uncounted, loaded };
}

/** The staged files to list: a conflict shows up on both sides, but it's resolved by staging it. */
export function stagedFiles(staged: ChangedFile[]): ChangedFile[] {
  return staged.filter((file) => file.status !== "conflicted");
}
