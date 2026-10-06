import type { ConflictSides } from "@gitto/git/types";
import {
  keepPreviousData,
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Loads a conflicted file's sides, and its text with the conflict markers if it's text. */
export function conflictQuery(repositoryId: string, path: string) {
  return queryOptions({
    queryKey: gitKeys.conflict(repositoryId, path),
    queryFn: ({ signal }) => rpc.git.conflicts.get({ repositoryId, path }, { signal }),
    // Only refetched once it's invalidated: like the status, whenever the working tree or the
    // index changes (see `filePatchQuery`).
    staleTime: Infinity,
  });
}

/**
 * A conflicted file (see `conflictQuery`), kept up to date as it's resolved. While another one's
 * loads, the last one's stays, as `isPlaceholderData` says: what's on show stays until it's in.
 */
export function useConflict(
  repositoryId: () => string,
  path: () => string,
  enabled: () => boolean,
) {
  return useQuery(() => ({
    ...conflictQuery(repositoryId(), path()),
    placeholderData: keepPreviousData,
    enabled: enabled(),
  }));
}

/** Resolving a conflicted file whole: keeping one side, or marking it resolved as it is. */
export type ConflictResolution =
  | {
      action: "keep";
      side: "ours" | "theirs";
      /** The sides the user saw, which have to be the file's still. */
      sides: ConflictSides;
      version: string | null;
    }
  | {
      action: "mark";
      version: string | null;
      /** Even with conflict markers left: the user said they belong in it. */
      withMarkers: boolean;
    };

/**
 * Resolves a conflicted file whole (see `ConflictResolution`). Settles once that's done, rather
 * than once the uncommitted changes are refetched after, so the view can move on at once.
 */
export function useResolveFile() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: ({
      repositoryId,
      path,
      resolution,
    }: {
      repositoryId: string;
      path: string;
      resolution: ConflictResolution;
    }) =>
      resolution.action === "keep"
        ? rpc.git.conflicts.keep({
            repositoryId,
            path,
            side: resolution.side,
            sides: resolution.sides,
            version: resolution.version,
          })
        : rpc.git.conflicts.markResolved({
            repositoryId,
            path,
            version: resolution.version,
            withMarkers: resolution.withMarkers,
          }),
    onSettled: (_result, _error, { repositoryId }) =>
      void queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId) }),
  }));
}
