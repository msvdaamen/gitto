import type { ChangedFile } from "@gitto/git/types";
import { keepPreviousData, useQuery, type QueryClient } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { diffFileKey, isUncommitted, type DiffSource } from "@/git/diff-source";
import { rpc } from "@/lib/rpc";

import { changedFilesQuery, fetchCommitFiles, stagedFiles } from "./diff";
import { gitKeys } from "./keys";
import { fetchStashFiles } from "./stash";
import { useUncommittedFiles } from "./status";
import { useUnsuspendedData } from "./unsuspended";

/**
 * The files whose changes `source` has, to step through, kept up to date for uncommitted ones.
 * Read without Suspense (see `useUnsuspendedData`): the uncommitted ones are refetched while
 * they're on show.
 */
export function useDiffSourceFiles(repositoryId: () => string, source: () => DiffSource) {
  const kind = () => source().kind;
  const sha = () => {
    const current = source();
    return isUncommitted(current) ? "" : current.sha;
  };
  const commit = useUnsuspendedData(
    useQuery(() =>
      changedFilesQuery(
        repositoryId(),
        sha(),
        gitKeys.commitFiles,
        fetchCommitFiles,
        kind() === "commit",
      ),
    ),
  );
  const stash = useUnsuspendedData(
    useQuery(() =>
      changedFilesQuery(
        repositoryId(),
        sha(),
        gitKeys.stashFiles,
        fetchStashFiles,
        kind() === "stash",
      ),
    ),
  );
  const workingTree = useUnsuspendedData(
    useUncommittedFiles(repositoryId, () => isUncommitted(source())),
  );
  const staged = createMemo(() => stagedFiles(workingTree()?.value.staged ?? []));
  const unstaged = () => workingTree()?.value.unstaged ?? [];

  const files = (): ChangedFile[] => {
    switch (kind()) {
      case "commit":
        return commit()?.files.value ?? [];
      case "stash":
        return stash()?.files.value ?? [];
      case "unstaged":
        return unstaged();
      case "staged":
        return staged();
    }
  };
  /** Whether `files` are `source`'s, rather than none yet or another commit's still on show. */
  const loaded = () => {
    switch (kind()) {
      case "commit":
        return commit()?.sha === sha();
      case "stash":
        return stash()?.sha === sha();
      default:
        return workingTree() !== undefined;
    }
  };
  /** The files' lines weren't counted, so one without line counts isn't known to be binary. */
  const uncounted = () => isUncommitted(source()) && (workingTree()?.value.uncounted ?? false);

  return { files, loaded, uncounted, staged, unstaged };
}

/** One file's patch in a source, with the file and its `diffFileKey`, so what's shown can match. */
export interface FilePatch {
  key: string;
  file: ChangedFile;
  patch: string;
}

/** Loads the patch of `file`'s changes in `source`. */
function filePatchQuery(repositoryId: string, source: DiffSource, file: ChangedFile) {
  const { path, origPath } = file;
  const fetchPatch = (signal: AbortSignal): Promise<string> => {
    switch (source.kind) {
      case "commit":
        return rpc.git.diff.commitFilePatch(
          { repositoryId, sha: source.sha, path, origPath },
          { signal },
        );
      case "stash":
        return rpc.git.stash.filePatch(
          { repositoryId, sha: source.sha, path, origPath },
          { signal },
        );
      case "unstaged":
        return rpc.git.diff.unstagedFilePatch(
          { repositoryId, path, origPath, untracked: file.status === "untracked" },
          { signal },
        );
      case "staged":
        return rpc.git.diff.stagedFilePatch({ repositoryId, path, origPath }, { signal });
    }
  };
  return {
    queryKey: isUncommitted(source)
      ? gitKeys.uncommittedFilePatch(repositoryId, source.kind, path)
      : source.kind === "commit"
        ? gitKeys.commitFilePatch(repositoryId, source.sha, path)
        : gitKeys.stashFilePatch(repositoryId, source.sha, path),
    queryFn: async ({ signal }: { signal: AbortSignal }): Promise<FilePatch> => ({
      key: diffFileKey(source, path),
      file,
      patch: await fetchPatch(signal),
    }),
    // Only refetched once it's invalidated: never for a commit's or a stash's, which don't change,
    // and for an uncommitted one (under `gitKeys.uncommitted`) like the status, whenever the working
    // tree or the index changes.
    staleTime: Infinity,
  };
}

/**
 * The patch of `file`'s changes in `source`. While another file's loads, the previous one's stays
 * on show rather than suspending: it comes with the file it's of.
 */
export function useFilePatch(
  repositoryId: () => string,
  source: () => DiffSource,
  file: () => ChangedFile,
  enabled: () => boolean,
) {
  return useQuery(() => ({
    ...filePatchQuery(repositoryId(), source(), file()),
    placeholderData: keepPreviousData,
    enabled: enabled(),
  }));
}

/** Loads the patch `useFilePatch` would, ahead of it, e.g. for a file about to be opened. */
export function fetchFilePatch(
  client: QueryClient,
  repositoryId: string,
  source: DiffSource,
  file: ChangedFile,
): Promise<FilePatch> {
  return client.fetchQuery(filePatchQuery(repositoryId, source, file));
}

/**
 * A file's contents in the working tree, to show more of its unstaged changes. Not cached: they can
 * change at any time, and are read only to fill in a patch that's just been loaded.
 */
export function fetchWorkingTreeFile(repositoryId: string, path: string): Promise<string> {
  return rpc.git.diff.workingTreeFile({ repositoryId, path });
}
