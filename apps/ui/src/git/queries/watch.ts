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
 * Refetches the repository's git data when it changes on disk; nothing else does (see
 * `queryClient`). The git directory (commits, checkouts, staging) is watched all the time. The
 * working tree is watched while the window has focus and for a while after; changes to it while
 * the window doesn't have focus are refetched once it gets focus back, and not at all if there
 * weren't any. A watch that stopped (e.g. Linux ran out of file watches) is started again, with a
 * refetch, when the window gets focus.
 */
export function useRepositoryWatcher(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const focused = useWindowFocus();

  createEffect(() => {
    const id = repositoryId();
    const refetchAll = () =>
      void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
    const refetchUncommitted = () =>
      void queryClient.invalidateQueries({ queryKey: gitKeys.uncommitted(id) });

    const gitDir = new Watch("the git directory", (signal) =>
      rpc.git.watch.gitDir({ repositoryId: id }, { signal }),
    );
    const workingTree = new Watch("the working tree", (signal) =>
      rpc.git.watch.workingTree({ repositoryId: id }, { signal }),
    );
    // Whether the working tree changed while the window didn't have focus.
    let changed = false;
    let unwatchTimer: ReturnType<typeof setTimeout> | undefined;

    const watchGitDir = () =>
      gitDir.start((changes: GitDirChange[]) => {
        if (changes.includes("refs")) refetchAll();
        else refetchUncommitted();
      });
    const watchWorkingTree = () =>
      workingTree.start(() => {
        if (focused()) refetchUncommitted();
        else changed = true;
      });

    // Nothing watched the repository until now, so what was loaded from it before (when it was last
    // on show) may be out of date. What's loading for the first time isn't loaded twice.
    refetchAll();
    watchGitDir();

    createEffect((wasFocused: boolean | undefined) => {
      if (!focused()) {
        unwatchTimer = setTimeout(() => workingTree.stop(), UNWATCH_AFTER_MS);
        return false;
      }
      clearTimeout(unwatchTimer);

      if (!gitDir.running) {
        // A commit or a checkout might have been missed, so everything's refetched.
        refetchAll();
        watchGitDir();
      } else if (workingTree.running ? changed : wasFocused === false) {
        // Without a working tree watch, anything might have changed; with one, only if it did.
        refetchUncommitted();
      }
      changed = false;
      if (!workingTree.running) watchWorkingTree();
      return true;
    });

    onCleanup(() => {
      clearTimeout(unwatchTimer);
      gitDir.stop();
      workingTree.stop();
    });
  });
}

/** A watch stream that can be started and stopped, and knows whether it's still running. */
class Watch<T> {
  private controller: AbortController | undefined;

  constructor(
    private readonly what: string,
    private readonly open: (signal: AbortSignal) => Promise<AsyncIterable<T>>,
  ) {}

  /** False once stopped, or once the stream ended by itself (an error, or the server closing it). */
  get running() {
    return this.controller !== undefined;
  }

  /** Calls `onEvent` for everything the stream emits, until it's stopped or ends. */
  start(onEvent: (event: T) => void) {
    const controller = new AbortController();
    this.controller = controller;
    const { signal } = controller;
    void (async () => {
      for await (const event of await this.open(signal)) onEvent(event);
    })()
      .catch((error: unknown) => {
        if (!signal.aborted) console.error(`Stopped watching ${this.what}`, error);
      })
      .finally(() => {
        if (this.controller === controller) this.controller = undefined;
      });
  }

  stop() {
    this.controller?.abort();
    this.controller = undefined;
  }
}
