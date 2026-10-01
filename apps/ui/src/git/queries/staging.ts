import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Files to stage or unstage: their paths (see `stagingPaths`), or all of them. */
export type StagingTarget = string[] | "all";

export function useStage(repositoryId: () => string) {
  return useStagingMutation(repositoryId, (id, target) =>
    target === "all"
      ? rpc.git.staging.stageAll({ repositoryId: id })
      : rpc.git.staging.stage({ repositoryId: id, paths: target }),
  );
}

export function useUnstage(repositoryId: () => string) {
  return useStagingMutation(repositoryId, (id, target) =>
    target === "all"
      ? rpc.git.staging.unstageAll({ repositoryId: id })
      : rpc.git.staging.unstage({ repositoryId: id, paths: target }),
  );
}

function useStagingMutation(
  repositoryId: () => string,
  run: (repositoryId: string, target: StagingTarget) => Promise<unknown>,
) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (target: StagingTarget) => run(repositoryId(), target),
    // The watcher would catch this too, but refetching right away feels snappier.
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(repositoryId()) }),
  }));
}
