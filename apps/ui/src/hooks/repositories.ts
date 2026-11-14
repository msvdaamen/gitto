import type { CloneProgress } from "@gitto/git/types";
import { useMutation, useQuery, useQueryClient } from "@tanstack/solid-query";
import { useNavigate } from "@tanstack/solid-router";
import { createMemo, createSignal, onCleanup } from "solid-js";

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
 * while it runs, and why it last failed, until that's dismissed or it's run again. Cancelling it
 * (or leaving the page that started it) stops git, which leaves nothing of the clone behind.
 */
export function useCloneRepository() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [running, setRunning] = createSignal(false);
  const [progress, setProgress] = createSignal<CloneProgress | null>(null);
  const [error, setError] = createSignal<Error | null>(null);
  let controller: AbortController | undefined;
  onCleanup(() => controller?.abort());

  /** Clones `url` into the folder `path`; `onCloned` runs once it's added, before it's opened. */
  async function clone(url: string, path: string, onCloned?: () => void) {
    if (running()) return;
    const current = (controller = new AbortController());
    setRunning(true);
    setProgress(null);
    setError(null);
    try {
      const steps = await rpc.git.clone.run({ url, path }, { signal: current.signal });
      let step = await steps.next();
      while (!step.done) {
        setProgress(step.value);
        // oxlint-disable-next-line no-await-in-loop -- each step comes after the last.
        step = await steps.next();
      }
      // The folder as git cloned into it, with `~` for the home folder spelled out.
      const repository = await rpc.repository.add({ path: step.value });
      await queryClient.invalidateQueries({ queryKey: REPOSITORIES_KEY });
      onCloned?.();
      void navigate({ to: "/$repoId", params: { repoId: repository.id } });
    } catch (failure) {
      // Cancelled: that's not a failure to show.
      if (current.signal.aborted) return;
      // The UI shows only the message; the stack and cause are for whoever looks into it.
      console.error("Couldn't clone the repository", failure);
      setError(failure instanceof Error ? failure : new Error(String(failure)));
    } finally {
      if (controller === current) controller = undefined;
      setRunning(false);
      setProgress(null);
    }
  }

  return {
    clone,
    /** Stops the clone that's running, if any. */
    cancel: () => controller?.abort(),
    running,
    /** How far the running clone has got, once git has said. */
    progress,
    error,
    /** Forgets why it last failed. */
    dismiss: () => setError(null),
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
