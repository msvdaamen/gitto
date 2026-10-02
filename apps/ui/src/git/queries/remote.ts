import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { createMemo, createSignal, onCleanup } from "solid-js";

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
  const cache = queryClient.getMutationCache();
  const mutation = useMutation(() => ({
    mutationKey: pullKey(repositoryId()),
    // The repository is passed in, rather than read when the pull ends, so the one pulled is
    // refreshed even if another one is on show by then.
    mutationFn: (id: string) => rpc.git.remote.pull({ repositoryId: id }),
    // Also after a failure: a pull that stopped at conflicts still brought in the upstream's commits.
    onSettled: (_data, _error, id) =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) }),
    // Kept until dismissed or pulled again, however long the user is in another repository.
    gcTime: Infinity,
  }));

  // The repository's pulls, again whenever the repository or one of them changes. Not through
  // `useMutationState`, which only reads its filters when the cache changes, not the repository.
  const [changes, setChanges] = createSignal(0);
  onCleanup(cache.subscribe(() => setChanges((count) => count + 1)));
  const pulls = createMemo(() => {
    changes();
    return cache.findAll({ mutationKey: pullKey(repositoryId()), exact: true });
  });
  const running = () => pulls().some((pull) => pull.state.status === "pending");
  const failed = () => pulls().filter((pull) => pull.state.status === "error");

  /** Forgets the repository's finished pulls, and so why they failed. */
  function forget() {
    for (const pull of pulls()) if (pull.state.status !== "pending") cache.remove(pull);
  }

  return {
    pull() {
      if (running()) return;
      forget();
      mutation.mutate(repositoryId());
    },
    isPending: running,
    error: () => failed().at(-1)?.state.error ?? null,
    dismiss: forget,
  };
}
