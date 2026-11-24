import { createEffect, createSignal } from "solid-js";

/**
 * `value`, but only true once it's been true for `ms`: for showing that something's in progress
 * without flashing when it's quick.
 */
export function useDelayed(value: () => boolean, ms = 300) {
  const [delayed, setDelayed] = createSignal(false);
  createEffect(value, (on) => {
    if (!on) {
      setDelayed(false);
      return;
    }
    const timer = setTimeout(() => setDelayed(true), ms);
    return () => clearTimeout(timer);
  });
  return delayed;
}
