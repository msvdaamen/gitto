import { render, screen } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { VirtualList, VirtualRows } from "./virtual-list";

const ITEMS = Array.from({ length: 100 }, (_, i) => `Row ${i}`);

describe("VirtualList", () => {
  beforeEach(() => {
    // jsdom has no layout: make every element 100px tall, so the list shows ten 10px rows.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(100);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(200);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("only renders the rows in view, plus a few past the edges", async () => {
    render(() => (
      <VirtualList items={ITEMS} rowHeight={10}>
        {(item) => <span>{item()}</span>}
      </VirtualList>
    ));

    await vi.waitFor(() => expect(screen.getByText("Row 0")).toBeInTheDocument());
    expect(screen.getByText("Row 19")).toBeInTheDocument();
    expect(screen.queryByText("Row 20")).not.toBeInTheDocument();
  });

  it("keeps the list as tall as all its rows, each at its own position", async () => {
    render(() => (
      <VirtualList items={ITEMS} rowHeight={10}>
        {(item) => <span>{item()}</span>}
      </VirtualList>
    ));

    const row = (await screen.findByText("Row 3")).parentElement!;
    expect(row).toHaveStyle({ height: "10px", transform: "translateY(30px)" });
    expect(row.parentElement).toHaveStyle({ height: "1000px" });
  });

  it("shows the items that replace the ones in view in the same rows", async () => {
    const [items, setItems] = createSignal(ITEMS);
    render(() => (
      <VirtualList items={items()} rowHeight={10}>
        {(item) => <span>{item()}</span>}
      </VirtualList>
    ));
    const row = await screen.findByText("Row 2");

    setItems(["Other 0", "Other 1", "Other 2"]);
    await vi.waitFor(() => expect(row).toHaveTextContent("Other 2"));
    expect(screen.queryByText("Row 3")).not.toBeInTheDocument();
  });
});

function renderRows(onNearEndChange?: (near: boolean) => void) {
  const [scroller, setScroller] = createSignal<HTMLDivElement>();
  render(() => (
    <div ref={setScroller}>
      <VirtualRows
        items={ITEMS}
        rowHeight={10}
        scrollElement={scroller()}
        nearEnd={20}
        onNearEndChange={onNearEndChange}
      >
        {(item, index) => <button data-row={index}>{item()}</button>}
      </VirtualRows>
    </div>
  ));
  return scroller;
}

describe("VirtualRows", () => {
  let scrollTop = 0;

  beforeEach(() => {
    // jsdom has no layout: make every element 100px tall, so the list shows ten 10px rows.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(100);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(200);
    vi.spyOn(HTMLElement.prototype, "scrollTop", "get").mockImplementation(() => scrollTop);
    scrollTop = 0;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("only renders the rows in view, plus a few past the edges", async () => {
    renderRows();
    await vi.waitFor(() => expect(screen.getByText("Row 0")).toBeInTheDocument());
    expect(screen.getByText("Row 19")).toBeInTheDocument();
    expect(screen.queryByText("Row 20")).not.toBeInTheDocument();
  });

  it("keeps the row with focus when it's scrolled out of view", async () => {
    const scroller = renderRows();
    const row = await screen.findByText("Row 0");
    row.focus();

    scrollTop = 500;
    scroller()!.dispatchEvent(new Event("scroll"));
    await vi.waitFor(() => expect(screen.getByText("Row 50")).toBeInTheDocument());
    expect(screen.queryByText("Row 1")).not.toBeInTheDocument();
    expect(screen.getByText("Row 0")).toBe(document.activeElement);

    // Once focus moves elsewhere, it goes like the others.
    row.blur();
    await vi.waitFor(() => expect(screen.queryByText("Row 0")).not.toBeInTheDocument());
  });

  it("says when the rows in view come near the end, and when they leave it", async () => {
    const onNearEndChange = vi.fn();
    const scroller = renderRows(onNearEndChange);
    await screen.findByText("Row 0");
    expect(onNearEndChange).toHaveBeenLastCalledWith(false);

    // Rows 60 to 69 in view, and ten more rendered past them: within 20 of the last.
    scrollTop = 600;
    scroller()!.dispatchEvent(new Event("scroll"));
    await vi.waitFor(() => expect(onNearEndChange).toHaveBeenLastCalledWith(true));
    expect(onNearEndChange).toHaveBeenCalledTimes(2);

    scrollTop = 0;
    scroller()!.dispatchEvent(new Event("scroll"));
    await vi.waitFor(() => expect(onNearEndChange).toHaveBeenLastCalledWith(false));
  });
});
