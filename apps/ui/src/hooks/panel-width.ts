import { createSignal } from "solid-js";

export type PanelBounds = { initial: number; min: number; max: number };

const storageKey = (name: string) => `gitto-panel-width:${name}`;

const clamp = (width: number, bounds: PanelBounds) =>
  Math.round(Math.min(bounds.max, Math.max(bounds.min, width)));

function load(name: string, bounds: PanelBounds): number {
  try {
    const value = Number(localStorage.getItem(storageKey(name)) ?? Number.NaN);
    return Number.isFinite(value) ? clamp(value, bounds) : bounds.initial;
  } catch {
    return bounds.initial;
  }
}

/**
 * The width of a resizable panel, kept within `bounds` and saved in `localStorage` so it survives
 * restarts. Shared by all repositories.
 */
export function usePanelWidth(name: string, bounds: PanelBounds) {
  const [width, setWidthSignal] = createSignal(load(name, bounds));

  const setWidth = (next: number) => {
    const value = clamp(next, bounds);
    setWidthSignal(value);
    try {
      localStorage.setItem(storageKey(name), String(value));
    } catch {
      // Storage full or unavailable: the width still applies until the app closes.
    }
  };

  return { width, setWidth, reset: () => setWidth(bounds.initial), bounds };
}
