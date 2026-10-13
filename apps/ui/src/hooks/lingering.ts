import { createMemo } from "solid-js";

/**
 * `value`, or the last one it had while it's `undefined`: what a dialog is about, still named
 * while it closes, rather than going blank as it fades. With `initial`, that's what it is before
 * it's had any.
 */
export function useLingering<T>(value: () => T | undefined, initial: T): () => T;
export function useLingering<T>(value: () => T | undefined): () => T | undefined;
export function useLingering<T>(value: () => T | undefined, initial?: T): () => T | undefined {
  return createMemo((last: T | undefined) => value() ?? last, initial);
}
