import type { GitDirChange } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import { createEffect, onCleanup } from "solid-js";

import { useWindowFocus } from "@/hooks/window-focus";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/**
 * How long the working tree stays watched after the window loses focus. Starting to watch walks
 * the whole tree, so switching to an editor and back shouldn't start over every time.
 */
export const UNWATCH_AFTER_MS = 5 * 60_000;

/**
 * Refetches the repository's git data when it changes on disk. The git directory (commits,
 * checkouts, staging) is watched all the time. The working tree is watched while the window has
 * focus and for a while after; changes to it while the window doesn't have focus are refetched
 * once it gets focus back, and not at all if there weren't any.
 */
export function useRepositoryWatcher(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const focused = useWindowFocus();

  const refetchUncommitted = (id: string) =>
    void queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(id) });

  createEffect(() => {
    const id = repositoryId();
    const controller = new AbortController();
    onCleanup(() => controller.abort());
    follow(
      "the git directory",
      (signal) => rpc.git.watch.gitDir({ repositoryId: id }, { signal }),
      controller.signal,
      (changes: GitDirChange[]) => {
        if (changes.includes("refs")) {
          void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
        } else {
          refetchUncommitted(id);
        }
      },
    );
  });

  createEffect(() => {
    const id = repositoryId();
    // Set while the working tree is watched.
    let watching: AbortController | undefined;
    // Whether it changed while the window didn't have focus.
    let changed = false;
    let unwatchTimer: ReturnType<typeof setTimeout> | undefined;

    function watch() {
      const controller = new AbortController();
      watching = controller;
      follow(
        "the working tree",
        (signal) => rpc.git.watch.workingTree({ repositoryId: id }, { signal }),
        controller.signal,
        () => {
          if (focused()) refetchUncommitted(id);
          else changed = true;
        },
        // E.g. Linux ran out of file watches: the next focus refetches and tries again.
        () => {
          if (watching === controller) watching = undefined;
        },
      );
    }
    function unwatch() {
      watching?.abort();
      watching = undefined;
    }

    createEffect((wasFocused: boolean | undefined) => {
      if (!focused()) {
        unwatchTimer = setTimeout(unwatch, UNWATCH_AFTER_MS);
        return false;
      }
      clearTimeout(unwatchTimer);
      // Without a watch, anything might have changed; with one, only refetch if something did.
      if (watching ? changed : wasFocused === false) refetchUncommitted(id);
      changed = false;
      if (!watching) watch();
      return true;
    });

    onCleanup(() => {
      clearTimeout(unwatchTimer);
      unwatch();
    });
  });
}

/** Calls `onEvent` for everything `open` streams, until `signal` aborts; `onError` if it fails. */
function follow<T>(
  what: string,
  open: (signal: AbortSignal) => Promise<AsyncIterable<T>>,
  signal: AbortSignal,
  onEvent: (event: T) => void,
  onError?: () => void,
) {
  void (async () => {
    for await (const event of await open(signal)) onEvent(event);
  })().catch((error: unknown) => {
    if (signal.aborted) return;
    console.error(`Stopped watching ${what}`, error);
    onError?.();
  });
}
