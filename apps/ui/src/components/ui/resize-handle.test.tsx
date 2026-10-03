import { fireEvent, render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { usePanelWidth } from "@/hooks/panel-width";

import { ResizeHandle } from "./resize-handle";

const BOUNDS = { initial: 220, min: 160, max: 480 };

function renderHandle(edge: "start" | "end") {
  let panel!: ReturnType<typeof usePanelWidth>;
  render(() => {
    panel = usePanelWidth("test", BOUNDS);
    return (
      <div>
        <ResizeHandle
          edge={edge}
          label="Resize panel"
          width={panel.width()}
          bounds={panel.bounds}
          onResize={panel.setWidth}
          onReset={panel.reset}
        />
      </div>
    );
  });
  return { panel, handle: screen.getByRole("separator", { name: "Resize panel" }) };
}

/** The width saved for the panel, if any. */
const saved = () => localStorage.getItem("gitto-panel-width:test");

afterEach(() => localStorage.clear());

describe("ResizeHandle", () => {
  it("nudges a panel on its left with the arrow keys", async () => {
    const user = userEvent.setup();
    const { panel, handle } = renderHandle("start");

    handle.focus();
    await user.keyboard("{ArrowRight}");
    expect(panel.width()).toBe(236);
    expect(handle).toHaveAttribute("aria-valuenow", "236");
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(panel.width()).toBe(204);
    await user.keyboard("{End}");
    expect(panel.width()).toBe(480);
    await user.keyboard("{Home}");
    expect(panel.width()).toBe(160);
  });

  it("grows a panel on its right with the left arrow key", async () => {
    const user = userEvent.setup();
    const { panel, handle } = renderHandle("end");

    handle.focus();
    await user.keyboard("{ArrowLeft}");
    expect(panel.width()).toBe(236);
  });

  it("saves the width it's dragged to once it's let go of", () => {
    // jsdom has no layout or pointer capture: the panel starts at the window's left edge.
    HTMLElement.prototype.setPointerCapture = () => undefined;
    HTMLElement.prototype.releasePointerCapture = () => undefined;
    const { panel, handle } = renderHandle("start");

    fireEvent.pointerDown(handle, { button: 0, clientX: 0, pointerId: 1 });
    fireEvent.pointerMove(handle, { clientX: 300, pointerId: 1 });
    expect(panel.width()).toBe(300);
    expect(saved()).toBeNull();
    fireEvent.pointerMove(handle, { clientX: 320, pointerId: 1 });
    expect(saved()).toBeNull();

    fireEvent.pointerUp(handle, { pointerId: 1 });
    expect(panel.width()).toBe(320);
    expect(saved()).toBe("320");
  });

  it("resets the width on double-click", () => {
    const { panel, handle } = renderHandle("start");
    panel.setWidth(400);

    fireEvent.dblClick(handle);
    expect(panel.width()).toBe(220);
  });
});
