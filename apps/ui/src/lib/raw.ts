/**
 * Holds a value Solid's stores leave alone. A query's result lives in a store, which walks through
 * every plain object and array in it each time the result changes, and wraps whatever is read from
 * them in a proxy: slow for a list of thousands of files. A class instance is stored as it is.
 */
export class Raw<T> {
  constructor(readonly value: T) {}
}
