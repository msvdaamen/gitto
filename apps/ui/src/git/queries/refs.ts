import type { Ref, RefList } from "@gitto/git/types";
import { useQuery, type QueryFunctionContext } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/**
 * The refs, in an object Solid's stores leave alone. A query's data is kept in a store, which
 * wraps every object in it in a proxy, and walks all of them whenever the data is replaced: with
 * the thousands of refs of a big repository (vscode has 5,800), that took 30ms of every refetch,
 * and reading them back through the proxies twice as long. Stores only wrap plain objects and
 * arrays, so the refs come in an instance of this class, and are only ever replaced as a whole.
 */
class Refs {
  constructor(readonly all: readonly Ref[]) {}
}

// Defined once, so a refetch that returns the same data doesn't select it again.
const selectRefs = (data: RefList) => new Refs(data.refs);

export function useRefs(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    const queryKey = gitKeys.refs(id);
    return {
      queryKey,
      queryFn: async ({ signal, client }: QueryFunctionContext) => {
        const previous = client.getQueryData<RefList>(queryKey);
        const result = await rpc.git.refs.list(
          { repositoryId: id, since: previous?.version },
          { signal },
        );
        // Unchanged: keep the same data, so nothing selected from it changes or renders again. Only
        // ever the answer when there was a previous list to compare with.
        return "unchanged" in result ? previous! : result;
      },
      select: selectRefs,
    };
  });
}
