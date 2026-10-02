import { useMutation, useQueryClient } from "@tanstack/solid-query";

import { rpc } from "@/lib/rpc";

import { commitQuery } from "./history";
import { gitKeys } from "./keys";

/** A commit to make: its message, and whether it replaces the last commit. */
export interface NewCommit {
  message: string;
  amend: boolean;
}

/** Commits what's staged, as described by the `NewCommit` passed to `mutate`. */
export function useCreateCommit(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (commit: NewCommit) =>
      rpc.git.commit.create({ repositoryId: repositoryId(), ...commit }),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: gitKeys.repository(repositoryId()) }),
  }));
}

/** Loads a commit's full message, e.g. the last one's to amend it. */
export function useCommitMessage(repositoryId: () => string) {
  const queryClient = useQueryClient();
  return async (sha: string) => {
    const commit = await queryClient.fetchQuery(commitQuery(repositoryId(), sha));
    return { summary: commit.subject, description: commit.body };
  };
}
