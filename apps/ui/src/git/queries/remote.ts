import { useMutation, useQueryClient } from "@tanstack/solid-query";
import { createStore, produce } from "solid-js/store";

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
 * Each repository's pull: whether one is running, and why the last one failed, until that's
 * dismissed or it's pulled again. Kept here rather than in a component, so it outlives the
 * toolbar showing it: switching repositories shows another one's, and the same one's again when
 * switching back. Set by the mutation's own callbacks, which run whatever is on show.
 */
const [pulls, setPulls] = createStore<Record<string, { running: boolean; error: Error | null }>>(
  {},
);

/** Pulls the current branch's upstream; whether a pull is running, and why the last one failed. */
export function usePull(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const mutation = useMutation(() => ({
    // The repository is passed in, rather than read when the pull ends, so the one pulled is
    // updated and refreshed even if another one is on show by then.
    mutationFn: (id: string) => rpc.git.remote.pull({ repositoryId: id }),
    onMutate: (id) => setPulls(id, { running: true, error: null }),
    onError: (error, id) => setPulls(id, { running: false, error }),
    // Nothing to keep for one that went through.
    onSuccess: (_data, id) => setPulls(produce((all) => delete all[id])),
    // Also after a failure: a pull that stopped at conflicts still brought in the upstream's commits.
    // Not awaited: the pull is done (and its error shown) before the history has reloaded.
    onSettled: (_data, _error, id) => {
      void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    },
  }));

  const pull = () => pulls[repositoryId()];
  return {
    pull() {
      if (pull()?.running) return;
      mutation.mutate(repositoryId());
    },
    isPending: () => !!pull()?.running,
    error: () => pull()?.error ?? null,
    /** Forgets why the last pull failed. */
    dismiss() {
      const id = repositoryId();
      if (!pulls[id]?.running) setPulls(produce((all) => delete all[id]));
    },
  };
}
