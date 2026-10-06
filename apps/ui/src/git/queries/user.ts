import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

/** The name the user commits with, from git's global config; `null` if it isn't set. */
export function useUserName() {
  return useQuery(() => ({
    queryKey: ["git-user-name"],
    queryFn: ({ signal }) => rpc.git.user.name(undefined, { signal }),
    // Not under `gitKeys`, so it doesn't get their defaults (see `queryClient`): without these,
    // `git config user.name` would run again every time the window got focus, for a name that's
    // read once and all but never changes.
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  }));
}
