import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";

import type { GraphRow } from "@/git/graph";

import { AuthorTooltipProvider } from "./author-tooltip";
import { HistoryGraph } from "./history-graph";

const row: GraphRow = {
  column: 0,
  through: [],
  top: [{ from: 0, to: 0 }],
  bottom: [{ from: 0, to: 0 }],
  width: 1,
};

describe("HistoryGraph", () => {
  it("shows the author in a tooltip while the commit's node is hovered", async () => {
    const { container } = render(() => (
      <AuthorTooltipProvider>
        <HistoryGraph row={row} author="Ada Lovelace" initials="AL" avatarColor="#8c65cf" />
      </AuthorTooltipProvider>
    ));
    const node = container.querySelector("svg g")!;
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    fireEvent.pointerEnter(node);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Ada Lovelace");

    fireEvent.pointerLeave(node);
    await waitFor(() => expect(screen.queryByRole("tooltip")).not.toBeInTheDocument());
  });

  it("hides the tooltip when the node is scrolled away from under the pointer", async () => {
    const { container } = render(() => (
      <AuthorTooltipProvider>
        <HistoryGraph row={row} author="Ada Lovelace" initials="AL" avatarColor="#8c65cf" />
      </AuthorTooltipProvider>
    ));
    fireEvent.pointerEnter(container.querySelector("svg g")!);
    expect(await screen.findByRole("tooltip")).toBeInTheDocument();

    fireEvent.scroll(container);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("doesn't show the tooltip of a node the pointer only passed over", async () => {
    vi.useFakeTimers();
    const { container } = render(() => (
      <AuthorTooltipProvider>
        <HistoryGraph row={row} author="Ada Lovelace" initials="AL" avatarColor="#8c65cf" />
      </AuthorTooltipProvider>
    ));
    const node = container.querySelector("svg g")!;
    fireEvent.pointerEnter(node);
    fireEvent.pointerLeave(node);
    await vi.advanceTimersByTimeAsync(1000);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    vi.useRealTimers();
  });

  it("has no tooltip on the uncommitted changes' node", () => {
    const { container } = render(() => <HistoryGraph row={row} wip />);
    expect(container.querySelector("svg g")).not.toBeInTheDocument();
  });
});
