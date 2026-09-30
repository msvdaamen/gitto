import type { Head } from "@gitto/git/types";
import { useQuery } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

export function useStatus(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.status(id),
      queryFn: ({ signal }) => rpc.git.status.get({ repositoryId: id }, { signal }),
    };
  });
}

/** What to call HEAD: the branch name, or "detached HEAD". */
export function headLabel(head: Head): string {
  return head.kind === "detached" ? "detached HEAD" : head.name;
}
