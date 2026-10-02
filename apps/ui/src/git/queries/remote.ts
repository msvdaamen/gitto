import type { Mutation } from "@tanstack/solid-query";
import {
  useIsMutating,
  useMutation,
  useMutationState,
  useQueryClient,
} from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

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

/**
 * A repository's pulls, in the mutation cache: their state outlives the toolbar showing it, which
 * shows another repository's when switching, and the same one's again when switching back.
 */
const pullKey = (repositoryId: string) => ["pull", repositoryId] as const;

/** Pulls the current branch's upstream; whether a pull is running, and why the last one failed. */
export function usePull(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const mutation = useMutation(() => ({
    mutationKey: pullKey(repositoryId()),
    // The repository is passed in, rather than read when the pull ends, so the one pulled is
    // refreshed even if another one is on show by then.
    mutationFn: (id: string) => rpc.git.remote.pull({ repositoryId: id }),
    // Also after a failure: a pull that stopped at conflicts still brought in the upstream's commits.
    onSettled: (_data, _error, id) =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) }),
  }));
  const running = useIsMutating(() => ({ mutationKey: pullKey(repositoryId()) }));
  const failed = useMutationState(() => ({
    filters: { mutationKey: pullKey(repositoryId()), status: "error" },
    select: (failure) => failure as Mutation<unknown, Error, string>,
  }));

  /** Forgets why the repository's pulls failed. */
  function dismiss() {
    for (const failure of failed()) queryClient.getMutationCache().remove(failure);
  }

  return {
    pull() {
      if (running() > 0) return;
      dismiss();
      mutation.mutate(repositoryId());
    },
    isPending: () => running() > 0,
    error: createMemo(() => failed().at(-1)?.state.error ?? null),
    dismiss,
  };
}
