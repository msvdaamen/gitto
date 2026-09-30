import { createMemo, createSignal } from "solid-js";

type CollapsedState = Record<string, boolean>;

const storageKey = (repositoryId: string) => `gitto-sidebar-collapsed:${repositoryId}`;

function load(repositoryId: string): CollapsedState {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(storageKey(repositoryId)) ?? "{}");
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as CollapsedState)
      : {};
  } catch {
    return {};
  }
}

/**
 * Which sidebar sections and folders are collapsed in a repository, saved in `localStorage` so it
 * survives restarts. Ids without a saved state fall back to `defaults`, then to expanded.
 */
export function useCollapsed(repositoryId: () => string, defaults: CollapsedState = {}) {
  // A fresh signal per repository, so switching repositories loads that repository's state.
  const state = createMemo(() => {
    const id = repositoryId();
    const [get, set] = createSignal(load(id));
    return { id, get, set };
  });

  const isCollapsed = (id: string) => state().get()[id] ?? defaults[id] ?? false;

  const toggle = (id: string) => {
    const { id: repository, get, set } = state();
    const next = { ...get(), [id]: !isCollapsed(id) };
    set(next);
    try {
      localStorage.setItem(storageKey(repository), JSON.stringify(next));
    } catch {
      // Storage full or unavailable: the state still applies until the app closes.
    }
  };

  return { isCollapsed, toggle };
}
