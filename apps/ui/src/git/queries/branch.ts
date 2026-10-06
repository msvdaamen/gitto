import type { Uncommitted } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";

import { branchName, isCheckedOut } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useRepositoryOperation, useRepositoryOperationState } from "./operation";

/**
 * `operation`, which switches branches, as one that doesn't run while a merge does: it'd switch
 * away from the branch merged into once that's done, before the user saw how it went. Whether it
 * can't run now, as it or a merge is running, is `isBlocked`.
 */
function notWhileMerging<T, R>(
  repositoryId: () => string,
  operation: ReturnType<typeof useRepositoryOperation<T, R>>,
) {
  const merging = useMergeBranchState(repositoryId);
  return {
    ...operation,
    run(input: T, options?: { onSuccess?: (result: R) => void }) {
      if (!merging.isPending()) operation.run(input, options);
    },
    isBlocked: () => operation.isPending() || merging.isPending(),
  };
}

/**
 * Creates a branch at HEAD and switches to it, taking the uncommitted changes along; whether it's
 * running, and why it last failed. It counts as running until the repository has reloaded, so the
 * toolbar names the new branch once it's done. Not while a merge runs.
 */
export function useCreateBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return notWhileMerging(
    repositoryId,
    useRepositoryOperation("branch", repositoryId, async (id, name: string) => {
      await rpc.git.branch.create({ repositoryId: id, name });
      await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }),
  );
}

const SWITCH = "switch";

/**
 * Switches to a branch, by its full ref name: for a remote one, the local branch tracking it, or a
 * new one that does. The uncommitted changes come along, or are kept in the stash when they
 * conflict with it. Whether it's running, and why it last failed. It counts as running until the
 * repository has reloaded, also after a failure: it may have switched and stashed. Switching to the
 * checked-out branch does nothing, rather than reloading the whole repository. Not while a merge
 * runs.
 */
export function useSwitchBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return notWhileMerging(
    repositoryId,
    useRepositoryOperation(SWITCH, repositoryId, async (id, ref: string) => {
      const head = queryClient.getQueryData<Uncommitted>(gitKeys.status(id))?.head;
      if (head && isCheckedOut(head, ref)) return;
      try {
        await rpc.git.branch.switch({ repositoryId: id, ref });
      } finally {
        await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
      }
    }),
  );
}

/**
 * Creates a branch at the branch `from`, by its full ref name, and switches to it, taking the
 * uncommitted changes along, or keeping them in the stash when they conflict with it. That's a
 * switch of branches, like `useSwitchBranch`'s: it's shown as one, and doesn't run alongside one.
 * It counts as running until the repository has reloaded, also after a failure: it may have
 * switched and stashed. Not while a merge runs.
 */
export function useCreateBranchFrom(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return notWhileMerging(
    repositoryId,
    useRepositoryOperation(
      SWITCH,
      repositoryId,
      async (id, input: { name: string; from: string }) => {
        try {
          await rpc.git.branch.create({ repositoryId: id, ...input });
        } finally {
          await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
        }
      },
    ),
  );
}

/**
 * Whether a switch of branches (see `useSwitchBranch` and `useCreateBranchFrom`) is running, and
 * why it last failed.
 */
export function useSwitchBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(SWITCH, repositoryId);
}

const MERGE = "merge";

/**
 * Merges a branch, by its full ref name, into the checked-out one, `into`, as the user saw it; the
 * merge's outcome, or why it failed. One that changed nothing fails, saying so: the branch had every
 * commit of it already. One that stopped at conflicts goes through, as `conflicts`, left for the
 * user to resolve. It counts as running until the repository has reloaded, also after a failure:
 * it may have stopped partway; it isn't reloaded after one that changed nothing.
 */
export function useMergeBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useRepositoryOperation(
    MERGE,
    repositoryId,
    async (id, input: { ref: string; into: string }) => {
      const reload = () => queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
      let outcome;
      try {
        outcome = await rpc.git.branch.merge({ repositoryId: id, ...input });
      } catch (error) {
        await reload();
        throw error;
      }
      // Nothing to reload when nothing changed.
      if (outcome === "up-to-date") {
        throw new Error(
          `Already up to date: ${input.into} has every commit of ${branchName(input.ref)}.`,
        );
      }
      await reload();
      return outcome;
    },
  );
}

/** Whether a merge (see `useMergeBranch`) is running, and why it last failed. */
export function useMergeBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(MERGE, repositoryId);
}
