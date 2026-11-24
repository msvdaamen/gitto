import { createEffect, createSignal, deep, untrack } from "solid-js";

/**
 * A query's data, read without suspending: `undefined` until it's first loaded, or if that failed,
 * and the last data it had while it's refetched. solid-query's `data` is an async value, whose read suspends the
 * nearest `Loading` until it's loaded and holds whatever depends on it while it's refetched. For
 * data refetched while it's on show, like the uncommitted changes, or shown outside a `Loading`,
 * it's copied by an effect instead, which waits for it without holding anything else.
 */
export function useUnsuspendedData<T>(query: { data: T; status: string }): () => T {
  // What it already has, if anything, so it's there from the start: a read that can't suspend,
  // once it has data, and deliberately not followed, as the effect below takes it from there.
  const [data, setData] = createSignal({
    value: untrack(() => (query.status === "success" ? deep(query.data) : undefined)) as T,
  });
  // A plain copy, as `data` is a store whose fields would each wait on another key's data.
  createEffect(() => deep(query.data), {
    effect: (value) => {
      setData({ value });
    },
    // A load that failed: nothing to show, rather than an error for an `Errored` boundary. The
    // query's `error` says why.
    error: () => {
      setData({ value: undefined as T });
    },
  });
  return () => data().value;
}
