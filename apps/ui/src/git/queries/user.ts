import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

/** The name the user commits with, from git's global config; `null` if it isn't set. */
export function useUserName() {
  return useQuery(() => ({
    queryKey: ["git-user-name"],
    queryFn: ({ signal }) => rpc.git.user.name(undefined, { signal }),
  }));
}
