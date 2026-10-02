import { queryOptions, useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Commits what's staged; with `amend`, HEAD's SHA, replaces that commit instead. */
export function useCreateCommit(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (commit: { message: string; amend?: string }) =>
      rpc.git.commit.create({ repositoryId: repositoryId(), ...commit }),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
    // A failure saying HEAD has moved means the status is behind; refetched without waiting for
    // it, so the error shows right away.
    onError: () => {
      void queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) });
    },
  }));
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
  });
}
