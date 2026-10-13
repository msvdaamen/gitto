import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";

import { hasCode } from "@/lib/errors";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { reloading, useRepositoryOperation, useRepositoryOperationState } from "./operation";

/** The remotes, by name. */
export function useRemotes(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.remotes(id),
      queryFn: ({ signal }) => rpc.git.remote.list({ repositoryId: id }, { signal }),
    };
  });
}

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

const PULL_BRANCH = "pull-branch";

/**
 * Pulls a local branch that isn't checked out, by its short name, fast-forwarding it to its
 * upstream without touching the working tree; whether that's running, and why it last failed. It
 * counts as running until the repository has reloaded, also after a failure: its remote was
 * fetched all the same.
 */
export function usePullBranch(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useRepositoryOperation(PULL_BRANCH, repositoryId, (id, branch: string) =>
    reloading(queryClient, id, rpc.git.remote.pull({ repositoryId: id, branch })),
  );
}

/** Whether a branch that isn't checked out is being pulled (see `usePullBranch`), and why that last failed. */
export function usePullBranchState(repositoryId: () => string) {
  return useRepositoryOperationState(PULL_BRANCH, repositoryId);
}

/**
 * Pushes the current branch, or force-pushes it; whether a push is running, and why the last one
 * failed. A push turned down as the remote branch has commits the branch doesn't fails with an
 * error `pushRejection` tells apart.
 */
export function usePush(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const push = useRepositoryOperation<{ force: boolean }>("push", repositoryId, (id, { force }) =>
    rpc.git.remote.push({ repositoryId: id, force }).finally(() => {
      // The remote-tracking branch moved, and so did how far ahead the branch is. Not awaited, as
      // for a pull.
      void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    }),
  );
  return {
    push: () => push.run({ force: false }),
    forcePush: () => push.run({ force: true }),
    isPending: push.isPending,
    error: push.error,
    dismiss: push.dismiss,
  };
}

/**
 * Why the remote branch turned a push down, if `error` says it has commits the branch doesn't:
 * `rewritten` when they're ones the branch had before a rebase or an amend, which force-pushing
 * replaces; `behind` when they're others, to pull.
 */
export function pushRejection(error: Error | null): "rewritten" | "behind" | undefined {
  if (hasCode(error, "PUSH_REWRITTEN")) return "rewritten";
  if (hasCode(error, "PUSH_BEHIND")) return "behind";
  return undefined;
}

/** Where the Push button pushes the current branch, and how many commits; or why it can't. */
export function pushTargetQuery(repositoryId: string) {
  return queryOptions({
    queryKey: gitKeys.pushTarget(repositoryId),
    queryFn: ({ signal }) => rpc.git.remote.pushTarget({ repositoryId }, { signal }),
    // Refetched when the refs or config change (it's under `repository(id)`), not on window focus.
    staleTime: Infinity,
  });
}
