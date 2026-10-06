import { createSignal } from "solid-js";

export type Theme = "dark" | "light";

const STORAGE_KEY = "gitto-theme";

function load(): Theme {
  try {
    return localStorage.getItem(STORAGE_KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

const [theme, setThemeSignal] = createSignal<Theme>(load());

// Sync the DOM and storage directly instead of via a module-level effect, which Solid can't dispose.
function applyTheme(next: Theme) {
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Storage full or unavailable: the theme still applies until the app closes.
  }
  setThemeSignal(next);
}

applyTheme(theme());

const toggleTheme = () => applyTheme(theme() === "dark" ? "light" : "dark");

/** The app's theme, saved in `localStorage` so it survives restarts. */
export const useTheme = () => ({ theme, toggleTheme });
