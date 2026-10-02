import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

export function useRefs(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.refs(id),
      queryFn: ({ signal }) => rpc.git.refs.list({ repositoryId: id }, { signal }),
    };
  });
}
