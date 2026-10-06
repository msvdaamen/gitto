import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The theme is read and applied when the module loads, so each test loads it anew.
beforeEach(() => vi.resetModules());
afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("useTheme", () => {
  it("starts from the saved theme, and saves a toggle", async () => {
    localStorage.setItem("gitto-theme", "light");
    const { useTheme } = await import("./theme");
    const { theme, toggleTheme } = useTheme();

    expect(theme()).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    toggleTheme();
    expect(theme()).toBe("dark");
    expect(localStorage.getItem("gitto-theme")).toBe("dark");
  });

  it("falls back to dark, and still toggles, without storage", async () => {
    const broken = {
      getItem: () => {
        throw new Error("Storage is unavailable");
      },
      setItem: () => {
        throw new Error("Storage is unavailable");
      },
    };
    vi.stubGlobal("localStorage", broken);
    const { useTheme } = await import("./theme");
    const { theme, toggleTheme } = useTheme();

    expect(theme()).toBe("dark");
    expect(() => toggleTheme()).not.toThrow();
    expect(theme()).toBe("light");
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});
