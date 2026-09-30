import { useQueryClient } from "@tanstack/solid-query";
import { createEffect, onCleanup } from "solid-js";

import { rpc } from "@/lib/rpc";

import { gitKeys } from "./keys";

/** Refetches the repository's git data whenever it changes on disk. */
export function useRepositoryWatcher(repositoryId: () => string) {
  const queryClient = useQueryClient();

  createEffect(() => {
    const id = repositoryId();
    const controller = new AbortController();
    onCleanup(() => controller.abort());

    void (async () => {
      const changes = await rpc.git.watch.changes(
        { repositoryId: id },
        { signal: controller.signal },
      );
      for await (const _ of changes) {
        void queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
      }
    })().catch((error: unknown) => {
      if (!controller.signal.aborted) console.error("Stopped watching repository", error);
    });
  });
}
