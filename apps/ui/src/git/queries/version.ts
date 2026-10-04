import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

export const GIT_VERSION_KEY = ["git-version"] as const;

/**
 * The installed git, and whether Gitto works with it. One that does is only checked once; one that
 * doesn't is checked again whenever the window gets focus, e.g. back from installing a newer one.
 */
export function useGitVersion() {
  return useQuery(() => ({
    queryKey: GIT_VERSION_KEY,
    queryFn: ({ signal }) => rpc.git.version.check(undefined, { signal }),
    staleTime: (query) => (query.state.data?.supported ? Infinity : 0),
  }));
}
