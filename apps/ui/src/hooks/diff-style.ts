import { createSignal } from "solid-js";

/** How a file's changes are laid out: in one column, or the old and new side by side. */
export type DiffStyle = "unified" | "split";

const STORAGE_KEY = "gitto-diff-style";

function load(): DiffStyle {
  try {
    return localStorage.getItem(STORAGE_KEY) === "split" ? "split" : "unified";
  } catch {
    return "unified";
  }
}

const [diffStyle, setDiffStyleSignal] = createSignal<DiffStyle>(load());

function setDiffStyle(next: DiffStyle) {
  setDiffStyleSignal(next);
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Storage full or unavailable: the style still applies until the app closes.
  }
}

/** The layout of the changes, saved in `localStorage` so it survives restarts. */
export const useDiffStyle = () => ({ diffStyle, setDiffStyle });
