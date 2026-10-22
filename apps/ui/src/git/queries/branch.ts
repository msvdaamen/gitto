import type { Uncommitted } from "@gitto/git/types";
import { useQueryClient, type QueryClient } from "@tanstack/solid-query";

import { branchName, isCheckedOut } from "@/git/status";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import {
  oneAtATime,
  reloading,
  useRepositoryOperation,
  useRepositoryOperationState,
} from "./operation";

const CREATE = "branch";
const SWITCH = "switch";
const MERGE = "merge";
const REBASE = "rebase";
const DELETE = "delete-branch";
/** Adding a worktree (see `useWorktreeActions`), which checks out a branch there, or creates one. */
export const ADD_WORKTREE = "add-worktree";

/**
 * Whether one of the operations that change the branches is running: creating one at HEAD, a
 * switch of branches (`useSwitchBranch`, `useCreateBranchFrom`), a merge, a rebase, deleting one,
 * or adding a worktree, which checks one out.
 */
export function useBranchesChanging(repositoryId: () => string): () => boolean {
  const states = [CREATE, SWITCH, MERGE, REBASE, DELETE, ADD_WORKTREE].map((name) =>
    useRepositoryOperationState(name, repositoryId),
  );
  return () => states.some((state) => state.isPending());
}

/**
 * `operation`, one of those that change the branches, as one that doesn't run while any of them
 * does: a switch would switch away from the branch merged into before the user saw how the merge
 * went, say, or check out a branch being deleted, which git then wouldn't delete. Whether it can't
 * run now, as one of them is running, is `isBlocked`.
 */
function oneBranchChangeAtATime<T, R>(
  repositoryId: () => string,
  operation: ReturnType<typeof useRepositoryOperation<T, R>>,
) {
  const isBlocked = useBranchesChanging(repositoryId);
  return { ...oneAtATime(operation, isBlocked), isBlocked };
}

/**
 * Creates a branch at HEAD and switches to it, taking the uncommitted changes along; whether it's
 * running, and why it last failed. It counts as running until the repository has reloaded, so the
 * toolbar names the new branch once it's done. Not while another change of the branches runs.
 */
export function useCreateBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return oneBranchChangeAtATime(
    repositoryId,
    useRepositoryOperation(CREATE, repositoryId, async (id, name: string) => {
      await rpc.git.branch.create({ repositoryId: id, name });
      await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }),
  );
}

/**
 * Switches to a branch, by its full ref name: for a remote one, the local branch tracking it, or a
 * new one that does. The uncommitted changes come along, or are kept in the stash when they
 * conflict with it. Whether it's running, and why it last failed. It counts as running until the
 * repository has reloaded, also after a failure: it may have switched and stashed. Switching to the
 * checked-out branch does nothing, rather than reloading the whole repository. Not while another
 * change of the branches runs.
 */
export function useSwitchBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return oneBranchChangeAtATime(
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
 * switch of branches, like `useSwitchBranch`'s: it's shown as one. It counts as running until the
 * repository has reloaded, also after a failure: it may have switched and stashed. Not while
 * another change of the branches runs.
 */
export function useCreateBranchFrom(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return oneBranchChangeAtATime(
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

/**
 * Deletes a branch, by its full ref name, with the commits only it has: a local one here only; a
 * remote one on its remote, and the local branches tracking it with it. Whether it's running, and
 * why it last failed. It counts as running until the repository has reloaded, also after a
 * failure: the remote one may be gone though the local ones aren't. Not while another change of
 * the branches runs.
 */
export function useDeleteBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return oneBranchChangeAtATime(
    repositoryId,
    useRepositoryOperation(DELETE, repositoryId, (id, ref: string) =>
      reloading(queryClient, id, rpc.git.branch.delete({ repositoryId: id, ref })),
    ),
  );
}

/** Whether a branch is being deleted (see `useDeleteBranch`), and why that last failed. */
export function useDeleteBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(DELETE, repositoryId);
}

/**
 * Merges a branch, by its full ref name, into the checked-out one, `into`, as the user saw it; the
 * merge's outcome, or why it failed. One that changed nothing fails, saying so: the branch had every
 * commit of it already. One that stopped at conflicts goes through, as `conflicts`, left for the
 * user to resolve. It counts as running until the repository has reloaded, also after a failure:
 * it may have stopped partway; it isn't reloaded after one that changed nothing. Not while another
 * change of the branches runs.
 */
export function useMergeBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return oneBranchChangeAtATime(
    repositoryId,
    useRepositoryOperation(MERGE, repositoryId, (id, input: { ref: string; into: string }) =>
      integrating(
        queryClient,
        id,
        rpc.git.branch.merge({ repositoryId: id, ...input }),
        `Already up to date: ${input.into} has every commit of ${branchName(input.ref)}.`,
      ),
    ),
  );
}

/**
 * Runs `action`, a merge or a rebase on the repository `id`, then reloads the repository, also
 * after a failure: it may have stopped partway. Not after one that changed nothing, which fails
 * instead, saying `upToDate`: the branch had every commit already.
 */
async function integrating<T extends string>(
  queryClient: QueryClient,
  id: string,
  action: Promise<T>,
  upToDate: string,
): Promise<T> {
  const reload = () => queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
  let outcome;
  try {
    outcome = await action;
  } catch (error) {
    await reload();
    throw error;
  }
  if (outcome === "up-to-date") throw new Error(upToDate);
  await reload();
  return outcome;
}

/** Whether a merge (see `useMergeBranch`) is running, and why it last failed. */
export function useMergeBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(MERGE, repositoryId);
}

/**
 * Rebases the checked-out branch, `branch`, as the user saw it, onto a branch, by its full ref
 * name; the rebase's outcome, or why it failed. One that changed nothing fails, saying so: the
 * branch was on top of the other already. One that stopped at conflicts goes through, as
 * `conflicts`, left for the user to resolve and continue. It counts as running until the
 * repository has reloaded, also after a failure: it may have stopped partway; it isn't reloaded
 * after one that changed nothing. Not while another change of the branches runs.
 */
export function useRebaseBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return oneBranchChangeAtATime(
    repositoryId,
    useRepositoryOperation(REBASE, repositoryId, (id, input: { ref: string; branch: string }) =>
      integrating(
        queryClient,
        id,
        rpc.git.branch.rebase({ repositoryId: id, ...input }),
        `Already up to date: ${input.branch} has every commit of ${branchName(input.ref)}.`,
      ),
    ),
  );
}

/** Whether a rebase (see `useRebaseBranch`) is running, and why it last failed. */
export function useRebaseBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(REBASE, repositoryId);
}
