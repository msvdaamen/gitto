import { createEffect, latest } from "solid-js";

/**
 * Runs `reset` whenever `value` changes, though not at first: for state that's of one repository,
 * like a dialog's fields or which item's menu is open, which another repository shouldn't get. As
 * soon as it changes, by `latest`: not once what it changed to has loaded, like another
 * repository's branches.
 */
export function useResetOn<T>(value: () => T, reset: () => void): void {
  createEffect(
    () => latest(value),
    () => {
      reset();
    },
    { defer: true },
  );
}
