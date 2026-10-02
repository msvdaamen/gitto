import {
  QueryObserver,
  useQueryClient,
  type QueryKey,
  type QueryObserverOptions,
  type QueryObserverResult,
} from "@tanstack/solid-query";
import { createComputed, createSignal, on, onCleanup, untrack } from "solid-js";

/**
 * A query's result as it is right now, for showing what's loaded without waiting for the rest.
 * Unlike with `useQuery`, reading `data` never suspends: it's `undefined` until the query is in.
 * The data isn't kept in a store either, so a new result is a new object.
 */
export function useQueryResult<
  TQueryFnData,
  TData = TQueryFnData,
  TKey extends QueryKey = QueryKey,
>(
  options: () => QueryObserverOptions<TQueryFnData, Error, TData, TQueryFnData, TKey>,
): () => QueryObserverResult<TData> {
  const client = useQueryClient();
  const defaulted = () => client.defaultQueryOptions(options());
  const observer = new QueryObserver(client, untrack(defaulted));
  const [result, setResult] = createSignal(observer.getCurrentResult());

  // Follows the options when they change, e.g. to another repository's query.
  createComputed(
    on(
      defaulted,
      (next) => {
        observer.setOptions(next);
        setResult(() => observer.getCurrentResult());
      },
      { defer: true },
    ),
  );
  onCleanup(observer.subscribe((next) => setResult(() => next)));

  return result;
}
