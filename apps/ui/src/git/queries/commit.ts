import { queryOptions, useMutation, useQueryClient, type QueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Commits what's staged; with `amend`, HEAD's SHA, replaces that commit instead. */
export function useCreateCommit(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (commit: { message: string; amend?: string }) =>
      rpc.git.commit.create({ repositoryId: repositoryId(), ...commit }),
    ...refreshAfterCommit(queryClient, repositoryId()),
  }));
}

/**
 * Rewrites the message of HEAD's commit, `sha`, leaving what's staged out of it; resolves to the
 * rewritten commit's SHA once the repository has reloaded.
 */
export function useRewordCommit(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (reword: { sha: string; message: string }) =>
      rpc.git.commit.reword({ repositoryId: repositoryId(), ...reword }),
    ...refreshAfterCommit(queryClient, repositoryId()),
  }));
}

/** What a commit's mutation reloads once it's done, or has failed. */
function refreshAfterCommit(queryClient: QueryClient, repositoryId: string) {
  return {
    onSuccess: () => queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId) }),
    // A failure saying HEAD has moved means the status is behind; refetched without waiting for
    // it, so the error shows right away.
    onError: () => {
      void queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId) });
    },
  };
}

/** A commit's full message, as written; it never changes. */
export function commitMessageQuery(repositoryId: string, sha: string) {
  return queryOptions({
    queryKey: gitKeys.commitMessage(repositoryId, sha),
    queryFn: ({ signal }) => rpc.git.commit.message({ repositoryId, sha }, { signal }),
    staleTime: Infinity,
  });
}

/** The remote branch `git push` would update that has the commit; `null` if it has no need to. */
export function pushedToQuery(repositoryId: string, sha: string) {
  return queryOptions({
    queryKey: gitKeys.pushedTo(repositoryId, sha),
    queryFn: ({ signal }) => rpc.git.commit.pushedTo({ repositoryId, sha }, { signal }),
    // Refetched when the refs change (it's under `repository(id)`), not on every window focus.
    staleTime: Infinity,
  });
}
