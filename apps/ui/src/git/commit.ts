import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

export function useCommit(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (message: string) =>
      rpc.git.commit.create({ repositoryId: repositoryId(), message }),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
  }));
}
