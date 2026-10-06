import { createSignal, onCleanup, onMount } from "solid-js";

/**
 * Whether `element` is on the page. The virtualizer takes the window to observe from the scroll
 * container's document when it first gets one, so it's only handed over once it's on the page: on
 * mount it can still be detached, being rendered inside a `Suspense` boundary.
 */
export function useConnected(element: () => HTMLElement | undefined) {
  const [connected, setConnected] = createSignal(false);
  onMount(() => {
    let frame = 0;
    const check = () => {
      if (element()?.isConnected) setConnected(true);
      else frame = requestAnimationFrame(check);
    };
    check();
    onCleanup(() => cancelAnimationFrame(frame));
  });
  return connected;
}
