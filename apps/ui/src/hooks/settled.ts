import { createEffect, createSignal, onCleanup, untrack } from "solid-js";

/**
 * `value`, but while it keeps changing, only its first and its last: a change after a quiet spell
 * goes through right away, and ones that follow within `ms` wait until it's been quiet for that
 * long. For what's costly to follow, like loading the selected commit's details while the
 * selection moves with a key held down. A change from or to `undefined` always goes through.
 */
export function useSettled<T>(value: () => T, ms = 120): () => T {
  // What it is when it's made, deliberately not followed: the effect below takes it from there.
  // Kept as it was last set too, as the signal only shows that once it's updated.
  let current = untrack(value);
  const [settled, setSettled] = createSignal({ value: current });
  const settle = (next: T) => {
    current = next;
    setSettled({ value: next });
  };
  let quietUntil = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  createEffect(
    value,
    (next) => {
      clearTimeout(timer);
      const now = performance.now();
      if (now >= quietUntil || next === undefined || current === undefined) {
        settle(next);
      } else {
        timer = setTimeout(() => settle(next), ms);
      }
      quietUntil = now + ms;
    },
    { defer: true },
  );
  onCleanup(() => clearTimeout(timer));

  return () => settled().value;
}
