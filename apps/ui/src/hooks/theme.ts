import { createSignal } from "solid-js";

export type Theme = "dark" | "light";

const [theme, setThemeSignal] = createSignal<Theme>(
  localStorage.getItem("gitto-theme") === "light" ? "light" : "dark",
);

// Sync the DOM and storage directly instead of via a module-level effect, which Solid can't dispose.
function applyTheme(next: Theme) {
  document.documentElement.dataset.theme = next;
  localStorage.setItem("gitto-theme", next);
  setThemeSignal(next);
}

applyTheme(theme());

const toggleTheme = () => applyTheme(theme() === "dark" ? "light" : "dark");

export const useTheme = () => ({ theme, toggleTheme });
