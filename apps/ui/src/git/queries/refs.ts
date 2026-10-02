import type { Ref } from "@gitto/git/types";
import { useQuery } from "@tanstack/solid-query";

import { Raw } from "@/lib/raw";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

// Defined once, so the same refs aren't wrapped again. A repository can have thousands of them.
const selectRefs = (refs: Ref[]): Raw<Ref[]> => new Raw(refs);

export function useRefs(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.refs(id),
      queryFn: ({ signal }) => rpc.git.refs.list({ repositoryId: id }, { signal }),
      select: selectRefs,
    };
  });
}
