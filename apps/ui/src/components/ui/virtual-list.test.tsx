import { render, screen } from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { VirtualList } from "./virtual-list";

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
        {(item) => <span>{item}</span>}
      </VirtualList>
    ));

    await vi.waitFor(() => expect(screen.getByText("Row 0")).toBeInTheDocument());
    expect(screen.getByText("Row 19")).toBeInTheDocument();
    expect(screen.queryByText("Row 20")).not.toBeInTheDocument();
  });

  it("keeps the list as tall as all its rows, each at its own position", async () => {
    render(() => (
      <VirtualList items={ITEMS} rowHeight={10}>
        {(item) => <span>{item}</span>}
      </VirtualList>
    ));

    const row = (await screen.findByText("Row 3")).parentElement!;
    expect(row).toHaveStyle({ height: "10px", transform: "translateY(30px)" });
    expect(row.parentElement).toHaveStyle({ height: "1000px" });
  });
});
