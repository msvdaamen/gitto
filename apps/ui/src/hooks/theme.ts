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

// What it was last set to, which the signal only shows once it's updated: toggling twice at once
// goes back to where it was.
let current = load();
const [theme, setThemeSignal] = createSignal<Theme>(current);

// Sync the DOM and storage directly instead of via a module-level effect, which Solid can't dispose.
function applyTheme(next: Theme) {
  current = next;
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Storage full or unavailable: the theme still applies until the app closes.
  }
  setThemeSignal(next);
}

applyTheme(current);

const toggleTheme = () => applyTheme(current === "dark" ? "light" : "dark");

/** The app's theme, saved in `localStorage` so it survives restarts. */
export const useTheme = () => ({ theme, toggleTheme });
