import type { GitDirChange } from "@gitto/git/types";
import { useQueryClient } from "@tanstack/solid-query";
import { createEffect, createSignal, onCleanup } from "solid-js";

import { useWindowFocus } from "@/hooks/window-focus";
import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";
import { useStatusNow } from "./status";

/**
 * How long the working tree stays watched after the window loses focus. Starting to watch walks
 * the whole tree, so switching to an editor and back shouldn't start over every time.
 */
export const UNWATCH_AFTER_MS = 5 * 60_000;

/**
 * Refetches the repository's git data when it changes on disk. The git directory (commits,
 * checkouts, staging) is watched all the time. The working tree is watched while the window has
 * focus and for a while after; changes to it while the window doesn't have focus are refetched
 * once it gets focus back, and not at all if there weren't any. A watch that stopped (e.g. Linux
 * ran out of file watches) is started again, with a refetch, when the window gets focus. Each time
 * the working tree watch starts, the uncommitted changes are refetched once it's ready: changes
 * from before that, like ones made while the status was being loaded, aren't reported.
 *
 * The data counts as up to date for as long as this watches (see `staleTime` in the query client);
 * once it stops, what's loaded is marked stale, so it's loaded again when the repository is opened
 * next.
 */
export function useRepositoryWatcher(repositoryId: () => string) {
  const queryClient = useQueryClient();
  const focused = useWindowFocus();
  const status = useStatusNow(repositoryId);

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
    // Whether the working tree is to be watched as soon as the status is in. Starting to watch
    // lists what git ignores, which walks the working tree like the status does: at the same time,
    // each takes a few times longer, and the status is what the UI is waiting for.
    const [starting, setStarting] = createSignal(false);

    const watchGitDir = () =>
      gitDir.start((changes: GitDirChange[]) => {
        if (changes.includes("refs")) refetchAll();
        else refetchUncommitted();
      });
    const watchWorkingTree = () =>
      // `changed`, or `ready`: whatever changed until then wasn't seen, so it's looked at again.
      workingTree.start(() => {
        if (focused()) refetchUncommitted();
        else changed = true;
      });

    watchGitDir();

    createEffect(() => {
      if (!focused()) {
        unwatchTimer = setTimeout(() => workingTree.stop(), UNWATCH_AFTER_MS);
        return;
      }
      clearTimeout(unwatchTimer);

      if (!gitDir.running) {
        // A commit or a checkout might have been missed, so everything's refetched.
        refetchAll();
        watchGitDir();
      } else if (workingTree.running && changed) {
        // Without a working tree watch, anything might have changed: the one started below
        // refetches once it's ready.
        refetchUncommitted();
      }
      changed = false;
      if (!workingTree.running) setStarting(true);
    });

    createEffect(() => {
      if (!starting() || status().isPending || status().isFetching) return;
      setStarting(false);
      if (!workingTree.running) watchWorkingTree();
    });

    onCleanup(() => {
      clearTimeout(unwatchTimer);
      gitDir.stop();
      workingTree.stop();
      // Not refetched now: nothing shows it anymore.
      void queryClient.invalidateQueries({
        queryKey: gitKeys.repository(id),
        refetchType: "none",
      });
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
