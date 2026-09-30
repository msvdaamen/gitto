import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { describe, expect, it } from "vitest";

import { SegmentedControl } from "./segmented-control";

describe("SegmentedControl", () => {
  it("selects the clicked option", async () => {
    const user = userEvent.setup();
    const [mode, setMode] = createSignal("List");
    render(() => (
      <SegmentedControl value={mode()} options={["List", "Agents"]} onChange={setMode} />
    ));

    const agents = screen.getByRole("button", { name: "Agents" });
    expect(agents).not.toHaveClass("bg-panel-hover");

    await user.click(agents);

    expect(mode()).toBe("Agents");
    expect(agents).toHaveClass("bg-panel-hover");
    expect(screen.getByRole("button", { name: "List" })).not.toHaveClass("bg-panel-hover");
  });
});
