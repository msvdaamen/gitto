import { createEffect, on } from "solid-js";

/**
 * Runs `reset` whenever `value` changes, though not at first: for state that's of one repository,
 * like a dialog's fields or which item's menu is open, which another repository shouldn't get.
 */
export function useResetOn<T>(value: () => T, reset: () => void): void {
  createEffect(on(value, () => reset(), { defer: true }));
}
