import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

export function useStatus(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.status(id),
      queryFn: ({ signal }) => rpc.git.status.get({ repositoryId: id }, { signal }),
      // Merged into the previous status, file by file, so a refetch only updates what changed:
      // otherwise every file is a new object and its row is rendered again.
      reconcile: "path",
    };
  });
}
