import { createEffect, createSignal, onCleanup } from "solid-js";

/**
 * `value`, but only true once it's been true for `ms`: for showing that something's in progress
 * without flashing when it's quick.
 */
export function useDelayed(value: () => boolean, ms = 300) {
  const [delayed, setDelayed] = createSignal(false);
  createEffect(() => {
    if (!value()) {
      setDelayed(false);
      return;
    }
    const timer = setTimeout(() => setDelayed(true), ms);
    onCleanup(() => clearTimeout(timer));
  });
  return delayed;
}
