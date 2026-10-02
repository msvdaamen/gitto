import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Pulls the upstream of the current branch of the repository passed to `mutate`. */
export function usePull() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    // The repository is passed in, rather than read when the pull ends, so the one pulled is
    // refreshed even if another one is on show by then.
    mutationFn: (repositoryId: string) => rpc.git.remote.pull({ repositoryId }),
    // Also after a failure: a pull that stopped at conflicts still brought in the upstream's commits.
    onSettled: (_data, _error, repositoryId) =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId) }),
  }));
}
