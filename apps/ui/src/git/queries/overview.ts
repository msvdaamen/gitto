import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { UNWATCHED } from "./watch";

/**
 * What the home page shows of a repository, beyond its status: where it's hosted, when it was last
 * fetched and committed to, and what was last done in it.
 */
export function useOverview(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.overview(id),
      queryFn: ({ signal }) => rpc.git.overview.get({ repositoryId: id }, { signal }),
      ...UNWATCHED,
    };
  });
}
