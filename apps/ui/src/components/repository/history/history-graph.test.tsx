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

    fireEvent.pointerLeave(node);
    await waitFor(() => expect(screen.queryByRole("tooltip")).not.toBeInTheDocument());
  });

  it("has no tooltip on the uncommitted changes' node", () => {
    const { container } = render(() => <HistoryGraph row={row} wip />);
    expect(container.querySelector("svg g")).not.toBeInTheDocument();
  });
});
