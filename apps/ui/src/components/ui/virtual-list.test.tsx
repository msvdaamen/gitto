import { fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { VirtualList, VirtualViewport } from "./virtual-list";

const ITEMS = Array.from({ length: 100 }, (_, i) => `Row ${i}`);

const shown = () => screen.queryAllByRole("listitem").map((row) => row.textContent);
const rows = (from: number, to: number) => ITEMS.slice(from, to + 1);

function List() {
  return (
    <VirtualList items={ITEMS} rowHeight={10}>
      {(item) => <li>{item}</li>}
    </VirtualList>
  );
}

/** The list in a scroll container; returns the container. */
function renderInViewport() {
  render(() => (
    <div data-testid="scroller">
      <VirtualViewport>
        <List />
      </VirtualViewport>
    </div>
  ));
  return screen.getByTestId("scroller");
}

describe("VirtualList", () => {
  it("renders every row outside a viewport", () => {
    render(() => <List />);

    expect(shown()).toEqual(ITEMS);
  });

  describe("in a viewport", () => {
    // jsdom has no layout: fake a 100px tall scroll container, scrolled `scrollTop` down.
    let scrollTop = 0;

    beforeEach(() => {
      scrollTop = 0;
      vi.stubGlobal(
        "ResizeObserver",
        class {
          observe() {}
          disconnect() {}
        },
      );
      vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(100);
      vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
        this: HTMLElement,
      ) {
        const top = this.dataset.testid === "scroller" ? 0 : -scrollTop;
        return DOMRect.fromRect({ y: top, height: 0 });
      });
    });

    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    it("only renders the rows in view, plus a few past the edges", () => {
      renderInViewport();

      expect(shown()).toEqual(rows(0, 19));
    });

    it("renders the rows scrolled into view, and keeps the list its full height", async () => {
      const scroller = renderInViewport();

      scrollTop = 500;
      fireEvent.scroll(scroller);

      await vi.waitFor(() => expect(shown()).toEqual(rows(40, 69)));
      const holder = screen.getByText("Row 40").parentElement!;
      expect(holder).toHaveStyle({ top: "400px" });
      expect(holder.parentElement).toHaveStyle({ height: "1000px" });
    });
  });
});
