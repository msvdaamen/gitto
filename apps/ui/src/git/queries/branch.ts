import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/**
 * Creates a branch at HEAD and switches to it, taking the uncommitted changes along. The repository
 * is in the mutation's `variables`, so one still running after switching repositories reloads the
 * one it was made in, and its error can be told apart from the current repository's.
 */
export function useCreateBranch() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (input: { repositoryId: string; name: string }) => rpc.git.branch.create(input),
    // Running until the repository has reloaded, so the toolbar names the new branch once it's done.
    onSuccess: (_, { repositoryId }) =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId) }),
  }));
}
