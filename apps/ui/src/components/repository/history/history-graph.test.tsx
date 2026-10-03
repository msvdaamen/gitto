import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";

import type { GraphRow } from "@/git/graph";

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
      <HistoryGraph row={row} author="Ada Lovelace" initials="AL" avatarColor="#8c65cf" />
    ));
    const node = container.querySelector("svg g")!;
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    fireEvent.pointerEnter(node);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Ada Lovelace");

    // The tooltip's own node took the hovered one's place, and is what the pointer then leaves.
    fireEvent.pointerLeave(container.querySelector("svg g")!);
    await waitFor(() => expect(screen.queryByRole("tooltip")).not.toBeInTheDocument());

    // Hovered again, it opens the way any tooltip does.
    fireEvent.pointerEnter(container.querySelector("svg g")!);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Ada Lovelace");
  });

  it("doesn't open when the pointer only passes over the node", async () => {
    const { container } = render(() => (
      <HistoryGraph row={row} author="Ada Lovelace" initials="AL" avatarColor="#8c65cf" />
    ));
    const node = container.querySelector("svg g")!;

    fireEvent.pointerEnter(node);
    fireEvent.pointerLeave(node);
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("has no tooltip on the uncommitted changes' node", () => {
    const { container } = render(() => <HistoryGraph row={row} wip />);
    expect(container.querySelector("svg g")).not.toBeInTheDocument();
  });
});
