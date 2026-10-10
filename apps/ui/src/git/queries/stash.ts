import { useQuery, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { useChangedFiles, type FilesFetcher } from "./diff";
import { gitKeys } from "./keys";
import { oneAtATime, reloading, useRepositoryOperation } from "./operation";

/** The stashes, newest first. */
export function useStashes(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.stashes(id),
      queryFn: ({ signal }) => rpc.git.stash.list({ repositoryId: id }, { signal }),
      // Merged into the previous list, stash by stash, so a refetch that brings nothing new (the
      // refs changed elsewhere, say) doesn't lay out and render the history again.
      reconcile: "sha",
    };
  });
}

/** Loads the files a stash changed, for `changedFilesQuery`. */
export const fetchStashFiles: FilesFetcher = (input, options) =>
  rpc.git.stash.files(input, options);

/** The files a stash changed compared to the commit it was made on, untracked ones included. */
export function useStashFiles(repositoryId: () => string, sha: () => string) {
  return useChangedFiles(repositoryId, sha, gitKeys.stashFiles, fetchStashFiles);
}

/**
 * Stashes every change, pops a stash, and drops one; whether each is running, and why it last
 * failed. Each counts as running until the repository has reloaded, so the stashes and changes on
 * show are never ones from before it. One runs at a time (`isPending`): the toolbar's pop names the
 * stash it pops, which has to be the newest. A pop of the newest, from the toolbar, and one of the
 * stash picked in a list are separate, to show each where it was started.
 */
export function useStashActions(repositoryId: () => string) {
  const queryClient = useQueryClient();
  // Each reloads the repository after, also after a failure: a pop can conflict.
  const popping = (id: string, sha: string) =>
    reloading(queryClient, id, rpc.git.stash.pop({ repositoryId: id, sha }));
  const operations = [
    useRepositoryOperation("stash", repositoryId, (id) =>
      reloading(queryClient, id, rpc.git.stash.push({ repositoryId: id })),
    ),
    useRepositoryOperation("pop", repositoryId, popping),
    useRepositoryOperation("pop-picked", repositoryId, popping),
    useRepositoryOperation("drop-stash", repositoryId, (id, sha: string) =>
      reloading(queryClient, id, rpc.git.stash.drop({ repositoryId: id, sha })),
    ),
  ] as const;
  const isPending = () => operations.some((operation) => operation.isPending());
  const [stash, pop, popPicked, drop] = operations;
  return {
    stash: oneAtATime(stash, isPending),
    /** Pops the newest stash, by its SHA. */
    pop: oneAtATime(pop, isPending),
    /** Pops the stash picked in a list, the newest or an older one. */
    popPicked: oneAtATime(popPicked, isPending),
    drop: oneAtATime(drop, isPending),
    /** Whether any of them is running. */
    isPending,
  };
}
