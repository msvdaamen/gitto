import { useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useRepositoryOperation } from "./operation";

/**
 * Creates a branch at HEAD and switches to it, taking the uncommitted changes along; whether it's
 * running, and why it last failed. It counts as running until the repository has reloaded, so the
 * toolbar names the new branch once it's done.
 */
export function useCreateBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useRepositoryOperation("branch", repositoryId, async (id, name: string) => {
    await rpc.git.branch.create({ repositoryId: id, name });
    await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
  });
}
