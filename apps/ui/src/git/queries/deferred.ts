import { createDeferred, deep } from "solid-js";

import { loadedData } from "./loaded";

/**
 * A query's data that lags rather than holds: while another key's data loads, the last data stays
 * on show, and whatever changed the key (like the selection) takes effect at once. Read directly,
 * a query's `data` holds every change that led to it until it's in. Its first load suspends the
 * nearest `Loading`; `undefined` if a load failed (see `loadedData`), which the query's `error`
 * says why. A plain copy, as `data` is a store whose fields would each wait on the new key's data.
 */
export function useDeferredData<T>(query: {
  data: T;
  isLoadingError: boolean;
}): () => T | undefined {
  return createDeferred(() => {
    const data = loadedData(query);
    return data === undefined ? undefined : deep(data);
  });
}
