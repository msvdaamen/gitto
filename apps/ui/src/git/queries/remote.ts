import { queryOptions, useMutation, useQueryClient } from "@tanstack/solid-query";

import { hasCode } from "@/lib/errors";
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
