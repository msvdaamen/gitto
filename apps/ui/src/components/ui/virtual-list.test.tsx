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
});

function renderRows() {
  const [scroller, setScroller] = createSignal<HTMLDivElement>();
  render(() => (
    <div ref={setScroller}>
      <VirtualRows items={ITEMS} rowHeight={10} scrollElement={scroller()}>
        {(item, index) => <button data-row={index()}>{item()}</button>}
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

    // Once focus moves elsewhere, it goes like the others: hidden, until another row needs it.
    row.blur();
    await vi.waitFor(() => expect(screen.getByText("Row 0")).not.toBeVisible());
  });

  it("reuses the rows that scroll out of view for the ones that scroll in", async () => {
    const scroller = renderRows();
    const row = await screen.findByText("Row 1");
    const rendered = () => scroller()!.querySelectorAll("button").length;
    expect(rendered()).toBe(20);

    scrollTop = 500;
    scroller()!.dispatchEvent(new Event("scroll"));
    await vi.waitFor(() => expect(screen.getByText("Row 50")).toBeInTheDocument());
    // The same element, now another row, at that row's place.
    expect(row).toBeVisible();
    expect(row).not.toHaveTextContent("Row 1");
    const index = Number(row.dataset.row);
    expect(row).toHaveTextContent(`Row ${index}`);
    expect(row.parentElement).toHaveStyle({ transform: `translateY(${index * 10}px)` });
    // Ten rows past each edge are rendered now, rather than past the bottom only.
    expect(rendered()).toBe(30);

    scrollTop = 0;
    scroller()!.dispatchEvent(new Event("scroll"));
    await vi.waitFor(() => expect(screen.getByText("Row 0")).toBeVisible());
    // The rows that aren't needed any more are kept, hidden, for when they are.
    expect(rendered()).toBe(30);
    const visible = [...scroller()!.querySelectorAll("button")].filter(
      (button) => button.parentElement!.style.display !== "none",
    );
    expect(visible.map((button) => button.textContent).toSorted()).toEqual(
      ITEMS.slice(0, 20).toSorted(),
    );
  });

  it("shows the items that replace the ones in view in the same rows", async () => {
    const [items, setItems] = createSignal(ITEMS);
    const [scroller, setScroller] = createSignal<HTMLDivElement>();
    render(() => (
      <div ref={setScroller}>
        <VirtualRows items={items()} rowHeight={10} scrollElement={scroller()}>
          {(item) => <button>{item()}</button>}
        </VirtualRows>
      </div>
    ));
    const row = await screen.findByText("Row 2");

    setItems(["Other 0", "Other 1", "Other 2"]);
    await vi.waitFor(() => expect(row).toHaveTextContent("Other 2"));
    expect(screen.getAllByRole("button")).toHaveLength(3);
  });
});
