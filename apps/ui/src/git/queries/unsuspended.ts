import { createEffect, createSignal, untrack } from "solid-js";

/**
 * A query's data, read without Suspense. solid-query reads it from a resource, and a computation
 * that does so before the resource first resolves (in the tick the query's made, say) is suspended
 * by every refetch after that, until it runs again. For data refetched while it's on show, like
 * the uncommitted changes, that takes everything under the nearest `Suspense` off the page and
 * puts it back, scrolled to the top. Effects never suspend, so the data is copied by one.
 */
export function useUnsuspendedData<T>(query: { data: T }): () => T {
  const [data, setData] = createSignal(untrack(() => query.data));
  createEffect(() => setData(() => query.data));
  return data;
}
