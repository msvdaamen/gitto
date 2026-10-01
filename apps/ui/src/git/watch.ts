import type { GitDirChange } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import { createEffect, onCleanup } from "solid-js";

import { useWindowFocus } from "@/hooks/window-focus";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** How often the uncommitted changes are checked while the window has focus. */
export const POLL_MS = 3000;

/**
 * Refetches the repository's git data when it changes on disk. The git directory (commits,
 * checkouts, staging) is watched all the time. Edits to files don't show up there, so the
 * uncommitted changes are refetched when the window gets focus, and polled while it has focus:
 * on big repositories, that's a lot cheaper than watching the whole working tree.
 */
export function useRepositoryWatcher(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const focused = useWindowFocus();

  const refetchUncommitted = (id: string) =>
    queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(id) });

  createEffect(() => {
    const id = repositoryId();
    const controller = new AbortController();
    onCleanup(() => controller.abort());

    void (async () => {
      const changes = await rpc.git.watch.gitDir(
        { repositoryId: id },
        { signal: controller.signal },
      );
      for await (const change of changes) refetch(change);
    })().catch((error: unknown) => {
      if (!controller.signal.aborted) console.error("Stopped watching the git directory", error);
    });

    function refetch(changes: GitDirChange[]) {
      if (changes.includes("refs")) {
        void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
      } else {
        void refetchUncommitted(id);
      }
    }
  });

  createEffect((wasFocused: boolean | undefined) => {
    const id = repositoryId();
    if (!focused()) return false;
    // Catch up on whatever changed while the window didn't have focus.
    if (wasFocused === false) void refetchUncommitted(id);

    // The next check is planned once the last one is done, so a slow repository isn't flooded.
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const poll = () => {
      timer = setTimeout(async () => {
        await refetchUncommitted(id);
        if (!stopped) poll();
      }, POLL_MS);
    };
    poll();
    onCleanup(() => {
      stopped = true;
      clearTimeout(timer);
    });
    return true;
  });
}
