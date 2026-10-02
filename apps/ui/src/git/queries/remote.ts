import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Pulls the current branch's upstream into it. */
export function usePull(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: () => rpc.git.remote.pull({ repositoryId: repositoryId() }),
    // Also after a failure: a pull that stopped at conflicts still brought in the upstream's commits.
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
  }));
}
