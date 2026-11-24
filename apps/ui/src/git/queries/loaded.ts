/**
 * A query's `data`, or `undefined` if its first load failed, for what shows that it failed itself
 * (with the query's `error`) rather than in an `Errored` boundary. Read then, `data` throws to the
 * nearest boundary, and with none, everything stops updating; whether it failed doesn't suspend,
 * so it's checked first. Until it's loaded, this suspends the nearest `Loading` as `data` does, and
 * a failed refetch keeps the data it had.
 */
export function loadedData<T>(query: { data: T; isLoadingError: boolean }): T | undefined {
  return query.isLoadingError ? undefined : query.data;
}

/**
 * Whether the query is loading, as its `isFetching` says, which reads its `data` too and so throws
 * as that does if its first load failed (see `loadedData`).
 */
export function isFetching(query: {
  isFetching: boolean;
  isLoadingError: boolean;
  fetchStatus: string;
}): boolean {
  return query.isLoadingError ? query.fetchStatus === "fetching" : query.isFetching;
}

/** Whether the query is loading again, as its `isRefetching` says, without throwing (see `isFetching`). */
export function isRefetching(query: {
  isFetching: boolean;
  isPending: boolean;
  isLoadingError: boolean;
  fetchStatus: string;
}): boolean {
  return isFetching(query) && !query.isPending;
}
