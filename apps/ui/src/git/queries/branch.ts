import type { Uncommitted } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useRepositoryOperation, useRepositoryOperationState } from "./operation";

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

const SWITCH = "switch";

/**
 * Switches to a branch, by its full ref name: for a remote one, the local branch tracking it, or a
 * new one that does. The uncommitted changes come along, or are kept in the stash when they
 * conflict with it. Whether it's running, and why it last failed. It counts as running until the
 * repository has reloaded, also after a failure: it may have switched and stashed. Switching to the
 * checked-out branch does nothing, rather than reloading the whole repository.
 */
export function useSwitchBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useRepositoryOperation(SWITCH, repositoryId, async (id, ref: string) => {
    const head = queryClient.getQueryData<Uncommitted>(gitKeys.status(id))?.head;
    if (head?.kind === "branch" && ref === `refs/heads/${head.name}`) return;
    try {
      await rpc.git.branch.switch({ repositoryId: id, ref });
    } finally {
      await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }
  });
}

/**
 * Creates a branch at the branch `from`, by its full ref name, and switches to it, taking the
 * uncommitted changes along, or keeping them in the stash when they conflict with it. That's a
 * switch of branches, like `useSwitchBranch`'s: it's shown as one, and doesn't run alongside one.
 * It counts as running until the repository has reloaded, also after a failure: it may have
 * switched and stashed.
 */
export function useCreateBranchFrom(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useRepositoryOperation(
    SWITCH,
    repositoryId,
    async (id, input: { name: string; from: string }) => {
      try {
        await rpc.git.branch.create({ repositoryId: id, ...input });
      } finally {
        await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
      }
    },
  );
}

/**
 * Whether a switch of branches (see `useSwitchBranch` and `useCreateBranchFrom`) is running, and
 * why it last failed.
 */
export function useSwitchBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(SWITCH, repositoryId);
}
