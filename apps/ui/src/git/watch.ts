import type { GitDirChange } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import { createEffect, onCleanup } from "solid-js";

import { useWindowFocus } from "@/hooks/window-focus";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/**
 * Refetches the repository's git data when it changes on disk. The git directory (commits,
 * checkouts, staging) is watched all the time. The working tree is only watched while the window
 * has focus, and checked again when it gets focus back: watching a big tree isn't free, and
 * nobody sees the changes in the meantime anyway.
 */
export function useRepositoryWatcher(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const focused = useWindowFocus();

  const refetchUncommitted = (id: string) =>
    void queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(id) });

  createEffect(() => {
    const id = repositoryId();
    follow(
      "the git directory",
      (signal) => rpc.git.watch.gitDir({ repositoryId: id }, { signal }),
      (changes: GitDirChange[]) => {
        if (changes.includes("refs")) {
          void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
        } else {
          refetchUncommitted(id);
        }
      },
    );
  });

  createEffect((wasFocused: boolean | undefined) => {
    const id = repositoryId();
    if (!focused()) return false;
    // Catch up on whatever changed while the working tree wasn't watched.
    if (wasFocused === false) refetchUncommitted(id);
    // If it fails (e.g. Linux ran out of file watches), the refetch on focus is the fallback.
    follow(
      "the working tree",
      (signal) => rpc.git.watch.workingTree({ repositoryId: id }, { signal }),
      () => refetchUncommitted(id),
    );
    return true;
  });
}

/** Calls `onEvent` for everything `open` streams, until the effect it's called in re-runs. */
function follow<T>(
  what: string,
  open: (signal: AbortSignal) => Promise<AsyncIterable<T>>,
  onEvent: (event: T) => void,
) {
  const controller = new AbortController();
  onCleanup(() => controller.abort());

  void (async () => {
    for await (const event of await open(controller.signal)) onEvent(event);
  })().catch((error: unknown) => {
    if (!controller.signal.aborted) console.error(`Stopped watching ${what}`, error);
  });
}
