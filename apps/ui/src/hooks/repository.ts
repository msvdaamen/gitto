import { useQuery } from "@tanstack/solid-query";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";

export const REPOSITORIES_KEY = ["repositories"] as const;

/** The repository as added to Gitto (name and path). */
export function useRepository(repositoryId: () => string) {
  const repositories = useQuery(() => ({
    queryKey: REPOSITORIES_KEY,
    queryFn: () => rpc.repository.list(),
  }));
  return createMemo(() =>
    repositories.data?.find((repository) => repository.id === repositoryId()),
  );
}
