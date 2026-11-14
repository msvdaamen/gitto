import type { CloneProgress } from "@gitto/git/types";
import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { useNavigate } from "@tanstack/solid-router";
import { createMemo, createSignal, onCleanup } from "solid-js";

import { overallProgress } from "@/git/clone";
import { rpc } from "@/lib/rpc";

export const REPOSITORIES_KEY = ["repositories"] as const;

/** A repository added to Gitto. */
export type Repository = Awaited<ReturnType<typeof rpc.repository.list>>[number];

/** The repositories added to Gitto (name and path), in the order of their tabs. */
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
    // The UI shows only the message; the stack and cause are for whoever looks into it.
    onError: (error) => console.error("Couldn't add the repository", error),
  }));
}

/**
 * Asks for a folder, adds the repository in it (see `useAddRepository`), and opens it; with why
 * that last failed (the folder isn't a repository, say), until it's dismissed or tried again.
 */
export function useOpenRepository() {
  const navigate = useNavigate();
  const addRepository = useAddRepository();
  return {
    open: () =>
      addRepository.mutate(undefined, {
        onSuccess: (repository) => {
          if (repository) void navigate({ to: "/$repoId", params: { repoId: repository.id } });
        },
      }),
    error: () => addRepository.error,
    /** Forgets why it last failed. */
    dismiss: () => addRepository.reset(),
  };
}

/**
 * Clones a repository into a folder, adds it to Gitto, and opens it: how far the clone has got
 * while it runs (see `overallProgress`), and why it last failed, until that's dismissed or it's run
 * again. Cancelling it (or leaving the page that started it) stops git, which leaves nothing of the
 * clone behind; once git is done, it can't be cancelled (`cancellable`), only added and opened.
 */
export function useCloneRepository() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [progress, setProgress] = createSignal<CloneProgress | null>(null);
  const [cancellable, setCancellable] = createSignal(false);
  let controller: AbortController | undefined;
  onCleanup(() => controller?.abort());

  const mutation = useMutation(() => ({
    /** The repository cloned and added; `null` if it was cancelled. */
    mutationFn: async ({ url, path }: { url: string; path: string }) => {
      const current = (controller = new AbortController());
      setProgress(null);
      setCancellable(true);
      try {
        const steps = await rpc.git.clone.run({ url, path }, { signal: current.signal });
        let step = await steps.next();
        while (!step.done) {
          const reported = step.value;
          setProgress((last) => overallProgress(last, reported));
          // oxlint-disable-next-line no-await-in-loop -- each step comes after the last.
          step = await steps.next();
        }
        setCancellable(false);
        // The folder as git cloned into it, with `~` for the home folder spelled out.
        const repository = await rpc.repository.add({ path: step.value });
        await queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY });
        return repository;
      } catch (error) {
        // Cancelled: that's not a failure to show.
        if (current.signal.aborted) return null;
        throw error;
      } finally {
        if (controller === current) controller = undefined;
        setCancellable(false);
        setProgress(null);
      }
    },
    // The UI shows only the message; the stack and cause are for whoever looks into it.
    onError: (error) => console.error("Couldn't clone the repository", error),
  }));

  return {
    /** Clones `url` into the folder `path`; `onCloned` runs once it's added, before it's opened. */
    clone(url: string, path: string, onCloned?: () => void) {
      if (mutation.isPending) return;
      mutation.mutate(
        { url, path },
        {
          onSuccess: (repository) => {
            if (!repository) return;
            onCloned?.();
            void navigate({ to: "/$repoId", params: { repoId: repository.id } });
          },
        },
      );
    },
    /** Stops the clone, while git is running it. */
    cancel: () => {
      if (cancellable()) controller?.abort();
    },
    running: () => mutation.isPending,
    /** Whether git is still cloning, so it can be cancelled, rather than it's being opened. */
    cancellable,
    /** How far the running clone has got, once git has said. */
    progress,
    error: () => mutation.error,
    /** Forgets why it last failed. */
    dismiss: () => mutation.reset(),
  };
}

/** Removes a repository from Gitto; the folder on disk is left untouched. */
export function useRemoveRepository() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (id: string) => rpc.repository.remove({ id }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY }),
    onError: (error) => console.error("Couldn't remove the repository", error),
  }));
}

/** Puts the repositories' tabs in the order of the ids it's given. */
export function useReorderRepositories() {
  const queryClient = useQueryClient();
  return useMutation(() => ({
    mutationFn: (ids: string[]) => rpc.repository.reorder({ ids }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY }),
    onError: (error) => console.error("Couldn't reorder the repositories", error),
  }));
}
