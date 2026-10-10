import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { useNavigate } from "@tanstack/solid-router";

import { REPOSITORIES_KEY, type Repository } from "@/hooks/repositories";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { oneAtATime, reloading, useRepositoryOperation } from "./operation";

/** The worktrees, the main one first; the one on show is marked `current`. */
export function useWorktrees(repositoryId: () => string) {
  return useQuery(() => {
    const id = repositoryId();
    return {
      queryKey: gitKeys.worktrees(id),
      queryFn: ({ signal }) => rpc.git.worktree.list({ repositoryId: id }, { signal }),
      // Merged into the previous list, worktree by worktree, so a refetch that brings nothing new
      // (the refs changed, say) doesn't lay out and render the history again.
      reconcile: "path",
    };
  });
}

/** What a worktree is added with: the folder, the branch (by its full ref name), and a new branch's name, if one is made from it. */
export interface AddWorktreeInput {
  path: string;
  branch: string;
  newBranch?: string;
}

const OPEN = "open-worktree";

/**
 * Opens a worktree of the repository, by its path: adds it to Gitto's repositories, as one of its
 * own, or finds it there, and shows it. Whether that's running, and why it last failed, wherever
 * it was started from (a worktree's menu, or a branch's).
 */
export function useOpenWorktree(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  return useRepositoryOperation(OPEN, repositoryId, async (_id, path: string) => {
    const repository = await rpc.repository.add({ path });
    await queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY });
    await navigate({ to: "/$repoId", params: { repoId: repository.id } });
  });
}

/**
 * Adds a worktree, and removes one; whether each is running, and why it last failed. Each counts
 * as running until the repository has reloaded, so the worktrees on show are never ones from
 * before it. One runs at a time (`isPending`), nor while one is being opened. A worktree that was
 * opened in Gitto is removed from its repositories too, as its folder is gone.
 */
export function useWorktreeActions(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const opening = useOpenWorktree(repositoryId);
  const operations = [
    useRepositoryOperation("add-worktree", repositoryId, (id, input: AddWorktreeInput) =>
      reloading(queryClient, id, rpc.git.worktree.add({ repositoryId: id, ...input })),
    ),
    useRepositoryOperation("remove-worktree", repositoryId, (id, path: string) =>
      reloading(queryClient, id, removeWorktree(id, path)),
    ),
  ] as const;
  /** Removes the worktree, and the repository it was opened as, if it was. */
  async function removeWorktree(id: string, path: string) {
    await rpc.git.worktree.remove({ repositoryId: id, path });
    const repositories =
      queryClient.getQueryData<Repository[]>(REPOSITORIES_KEY) ?? (await rpc.repository.list());
    const opened = repositories.find((repository) => repository.path === path);
    if (!opened) return;
    await rpc.repository.remove({ id: opened.id });
    await queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY });
  }
  const isPending = () =>
    opening.isPending() || operations.some((operation) => operation.isPending());
  const [add, remove] = operations;
  return {
    add: oneAtATime(add, isPending),
    remove: oneAtATime(remove, isPending),
    open: oneAtATime(opening, isPending),
    /** Whether any of them is running. */
    isPending,
  };
}
