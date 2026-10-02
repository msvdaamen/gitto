import { queryOptions, useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Commits what's staged; with `amend`, replaces the last commit instead. */
export function useCreateCommit(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (commit: { message: string; amend: boolean }) =>
      rpc.git.commit.create({ repositoryId: repositoryId(), ...commit }),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
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
