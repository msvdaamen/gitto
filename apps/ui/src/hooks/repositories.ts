import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { useNavigate } from "@tanstack/solid-router";
import { createMemo } from "solid-js";

import { rpc } from "@/lib/rpc";

export const REPOSITORIES_KEY = ["repositories"] as const;

/** A repository added to Gitto. */
export type Repository = Awaited<ReturnType<typeof rpc.repository.list>>[number];

/** The repositories added to Gitto (name and path), in the order they were added. */
export function useRepositories() {
  return useQuery(() => ({
    queryKey: REPOSITORIES_KEY,
    queryFn: () => rpc.repository.list(),
    // Merged into the previous list by id, so a refetch keeps the unchanged entries' objects and
    // whatever is rendered for them.
    reconcile: "id",
  }));
}

/** The repository as added to Gitto (name and path). */
export function useRepository(repositoryId: () => string) {
  const repositories = useRepositories();
  return createMemo(() =>
    repositories.data?.find((repository) => repository.id === repositoryId()),
  );
}

/**
 * Asks for a folder and adds the repository in it; resolves to the repository, or `null` when the
 * folder picker was cancelled.
 */
export function useAddRepository() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: async () => {
      const path = await rpc.system.selectFolder();
      return path ? rpc.repository.add({ path }) : null;
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY }),
  }));
}

/** Asks for a folder, adds the repository in it (see `useAddRepository`), and opens it. */
export function useOpenRepository() {
  const navigate = useNavigate();
  const addRepository = useAddRepository();
  return () =>
    addRepository.mutate(undefined, {
      onSuccess: (repository) => {
        if (repository) void navigate({ to: "/$repoId", params: { repoId: repository.id } });
      },
      onError: (error) => console.error("Failed to add repository", error),
    });
}

/** Removes a repository from Gitto; the folder on disk is left untouched. */
export function useRemoveRepository() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (id: string) => rpc.repository.remove({ id }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY }),
  }));
}
