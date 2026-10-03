import { createRoot } from "solid-js";
import { afterEach, describe, expect, it } from "vitest";

import { usePanelWidth } from "./panel-width";

const BOUNDS = { initial: 220, min: 160, max: 480 };

/** Runs the hook in its own reactive root, like a freshly mounted layout. */
function mount() {
  return createRoot((dispose) => ({ ...usePanelWidth("sidebar", BOUNDS), dispose }));
}

afterEach(() => localStorage.clear());

describe("usePanelWidth", () => {
  it("starts at the initial width", () => {
    const panel = mount();

    expect(panel.width()).toBe(220);
    panel.dispose();
  });

  it("keeps the width within its bounds", () => {
    const panel = mount();

    panel.setWidth(1000);
    expect(panel.width()).toBe(480);
    panel.setWidth(20);
    expect(panel.width()).toBe(160);
    panel.dispose();
  });

  it("keeps the width across remounts, until it's reset", () => {
    const first = mount();
    first.setWidth(300.4);
    first.dispose();

    const second = mount();
    expect(second.width()).toBe(300);
    second.reset();
    second.dispose();

    const third = mount();
    expect(third.width()).toBe(220);
    third.dispose();
  });

  it("doesn't save a width that's still being dragged", () => {
    const first = mount();
    first.setWidth(300, false);
    expect(first.width()).toBe(300);
    first.dispose();
    expect(mount().width()).toBe(220);

    const second = mount();
    second.setWidth(300, false);
    second.setWidth(310);
    second.dispose();
    expect(mount().width()).toBe(310);
  });

  it("ignores unreadable saved widths", () => {
    localStorage.setItem("gitto-panel-width:sidebar", "wide");
    const panel = mount();

    expect(panel.width()).toBe(220);
    panel.dispose();
  });
});
