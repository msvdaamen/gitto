import type { ChangedFile } from "@gitto/git/types";
import { keepPreviousData, useQuery, type QueryClient, type QueryKey } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { lineTotals } from "@/git/changes";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { Opaque } from "./opaque";
import { useUncommittedFiles } from "./status";

/** Loads the files a commit or stash changed. */
type FilesFetcher = (
  input: { repositoryId: string; sha: string },
  options: { signal: AbortSignal },
) => Promise<ChangedFile[]>;

/**
 * The files the commit or stash `sha` changed, which never change, as `fetch` loads them under
 * `queryKey`. While another one's load, the previous ones stay on show rather than suspending;
 * `shownSha` says whose they are, so what's shown with them can match.
 */
export function useChangedFiles(
  repositoryId: () => string,
  sha: () => string,
  queryKey: (repositoryId: string, sha: string) => QueryKey,
  fetch: FilesFetcher,
) {
  const query = useQuery(() => {
    const id = repositoryId();
    const target = sha();
    return {
      queryKey: queryKey(id, target),
      queryFn: async ({ signal }: { signal: AbortSignal }) => ({
        sha: target,
        // A commit can change tens of thousands of files.
        files: new Opaque(await fetch({ repositoryId: id, sha: target }, { signal })),
      }),
      staleTime: Infinity,
      placeholderData: keepPreviousData,
    };
  });

  const files = createMemo(() => query.data?.files.value ?? []);
  const totals = createMemo(() => lineTotals(files()));
  /** The SHA whose files are on show: the previous one's, while `sha`'s load. */
  const shownSha = () => query.data?.sha;

  return { query, files, totals, shownSha };
}

/** Files changed by a commit, compared to its first parent. */
export function useCommitFiles(repositoryId: () => string, sha: () => string) {
  return useChangedFiles(repositoryId, sha, gitKeys.commitFiles, (input, options) =>
    rpc.git.diff.commitFiles(input, options),
  );
}

/** The patch of one file a commit changed, compared to its first parent, with what it's of. */
function commitFilePatchQuery(repositoryId: string, sha: string, file: ChangedFile) {
  return {
    queryKey: gitKeys.commitFilePatch(repositoryId, sha, file.path),
    queryFn: async ({ signal }: { signal: AbortSignal }) => ({
      sha,
      file,
      patch: await rpc.git.diff.commitFilePatch(
        { repositoryId, sha, path: file.path, origPath: file.origPath },
        { signal },
      ),
    }),
    // It never changes.
    staleTime: Infinity,
  };
}

/**
 * The patch of one file a commit changed, compared to its first parent. While another file's
 * loads, the previous one's stays on show rather than suspending: it comes with the commit and
 * file it's of.
 */
export function useCommitFilePatch(
  repositoryId: () => string,
  sha: () => string,
  file: () => ChangedFile,
  enabled: () => boolean,
) {
  return useQuery(() => ({
    ...commitFilePatchQuery(repositoryId(), sha(), file()),
    placeholderData: keepPreviousData,
    enabled: enabled(),
  }));
}

/** Loads the patch `useCommitFilePatch` would, ahead of it, e.g. for a file about to be opened. */
export function fetchCommitFilePatch(
  client: QueryClient,
  repositoryId: string,
  sha: string,
  file: ChangedFile,
) {
  return client.fetchQuery(commitFilePatchQuery(repositoryId, sha, file));
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

  // A conflict shows up on both sides, but it's resolved (and so staged) by staging it.
  const staged = createMemo(() =>
    (query.data?.value.staged ?? []).filter((file) => file.status !== "conflicted"),
  );
  const unstaged = createMemo(() => query.data?.value.unstaged ?? []);
  /** Whether there are too many files for their lines to have been counted. */
  const uncounted = () => query.data?.value.uncounted ?? false;

  return { query, staged, unstaged, uncounted };
}
