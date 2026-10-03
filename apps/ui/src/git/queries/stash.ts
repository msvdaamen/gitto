import { useQuery, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useRepositoryOperation } from "./operation";

/** The stashes, newest first. */
export function useStashes(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.stashes(id),
      queryFn: ({ signal }) => rpc.git.stash.list({ repositoryId: id }, { signal }),
    };
  });
}

/**
 * Stashes every change, and pops the newest stash; whether each is running, and why it last
 * failed. Each counts as running until the repository has reloaded, so the stashes and changes on
 * show are never ones from before it: a pop names the stash it pops, which has to be the newest.
 */
export function useStashActions(repositoryId: () => string) {
  const queryClient = useQueryClient();
  /** Runs `action`, then reloads the repository, also after a failure: a pop can conflict. */
  const reloading = async (id: string, action: Promise<unknown>) => {
    try {
      await action;
    } finally {
      await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }
  };
  return {
    stash: useRepositoryOperation("stash", repositoryId, (id) =>
      reloading(id, rpc.git.stash.push({ repositoryId: id })),
    ),
    pop: useRepositoryOperation("pop", repositoryId, (id, sha: string) =>
      reloading(id, rpc.git.stash.pop({ repositoryId: id, sha })),
    ),
  };
}
