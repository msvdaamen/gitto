import { createEffect, createSignal, on, onCleanup } from "solid-js";

/**
 * `value`, but while it keeps changing, only its first and its last: a change after a quiet spell
 * goes through right away, and ones that follow within `ms` wait until it's been quiet for that
 * long. For what's costly to follow, like loading the selected commit's details while the
 * selection moves with a key held down. A change from or to `undefined` always goes through.
 */
export function useSettled<T>(value: () => T, ms = 120): () => T {
  const [settled, setSettled] = createSignal(value());
  let quietUntil = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  createEffect(
    on(
      value,
      (next) => {
        clearTimeout(timer);
        const now = performance.now();
        if (now >= quietUntil || next === undefined || settled() === undefined) {
          setSettled(() => next);
        } else {
          timer = setTimeout(() => setSettled(() => next), ms);
        }
        quietUntil = now + ms;
      },
      { defer: true },
    ),
  );
  onCleanup(() => clearTimeout(timer));

  return settled;
}
