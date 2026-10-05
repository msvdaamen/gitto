import type { UpdateState } from "@gitto/system/types";
import { createSignal, onCleanup } from "solid-js";

import { rpc } from "@/lib/rpc";

/** The running Gitto's version, and the update it's getting; `undefined` until the main process says. */
export function useUpdate() {
  const [state, setState] = createSignal<UpdateState>();
  const controller = new AbortController();
  const { signal } = controller;
  void (async () => {
    for await (const next of await rpc.system.update.watch(undefined, { signal })) setState(next);
  })().catch((error: unknown) => {
    if (!signal.aborted) console.error("Stopped watching for updates", error);
  });
  onCleanup(() => controller.abort());
  return state;
}
