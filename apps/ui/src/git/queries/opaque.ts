/**
 * A value in a query's result that Solid's store leaves alone. A query's result is kept in a store,
 * which wraps what it's given to track every field read from it, and walks all of it each time the
 * query's state changes. For a list of thousands of files that takes long enough to stall the UI,
 * and gains nothing: the list is replaced as a whole when it changes, never changed in place. The
 * store only does so for plain objects and arrays, which this isn't.
 */
export class Opaque<T> {
  constructor(readonly value: T) {}
}
