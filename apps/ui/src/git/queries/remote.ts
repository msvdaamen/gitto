import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useRepositoryOperation } from "./operation";

/**
 * Fetches every remote of the repository whose id is passed to `mutate`. The id is the mutation's
 * `variables`, so a fetch still running after switching repositories refetches the one it fetched,
 * and its state can be told apart from the current repository's.
 */
export function useFetch() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (repositoryId: string) => rpc.git.remote.fetch({ repositoryId }),
    // The watcher would catch the new remote refs too, but refetching right away feels snappier.
    onSuccess: (_, repositoryId) =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId) }),
  }));
}

/** Pulls the current branch's upstream; whether a pull is running, and why the last one failed. */
export function usePull(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const pull = useRepositoryOperation("pull", repositoryId, (id) =>
    rpc.git.remote.pull({ repositoryId: id }).finally(() => {
      // Also after a failure: a pull that stopped at conflicts still brought in the upstream's
      // commits. Not awaited: the pull is done (and its error shown) before the history has
      // reloaded.
      void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }),
  );
  return {
    pull: () => pull.run(),
    isPending: pull.isPending,
    error: pull.error,
    dismiss: pull.dismiss,
  };
}
