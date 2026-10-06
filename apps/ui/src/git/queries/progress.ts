import type { OperationKind } from "@gitto/git/types";
import { useQuery, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useRepositoryOperation } from "./operation";

/** The merge, rebase, cherry-pick, revert or `git am` under way in the repository; `null` if none. */
export function useOperationInProgress(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.operation(id),
      queryFn: ({ signal }) => rpc.git.operation.get({ repositoryId: id }, { signal }),
    };
  });
}

/**
 * Continues or aborts the operation under way; whether that's running, and why it last failed.
 * Each counts as running until the repository has reloaded, so the operation on show is never the
 * one from before: a rebase that goes on stops at the next commit's conflicts, say.
 */
export function useOperationActions(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const reloading = async (id: string, action: Promise<unknown>) => {
    try {
      await action;
    } finally {
      await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }
  };
  return {
    continue: useRepositoryOperation("continue", repositoryId, (id, kind: OperationKind) =>
      reloading(id, rpc.git.operation.continue({ repositoryId: id, kind })),
    ),
    abort: useRepositoryOperation("abort", repositoryId, (id, kind: OperationKind) =>
      reloading(id, rpc.git.operation.abort({ repositoryId: id, kind })),
    ),
  };
}
