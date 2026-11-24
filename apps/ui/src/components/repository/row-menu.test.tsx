import Check from "@lucide/solid/icons/check";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal, flush } from "solid-js";
import { describe, expect, it, vi } from "vitest";

import { ContextMenuItem } from "@/components/ui/context-menu";

import { RowMenu } from "./row-menu";

describe("RowMenu", () => {
  it("closes, and forgets which item it was open for, once another repository is switched to", async () => {
    const user = userEvent.setup();
    const [repositoryId, setRepositoryId] = createSignal("a");
    const onOpenFor = vi.fn();
    render(() => (
      <RowMenu
        repositoryId={repositoryId()}
        attribute="item"
        item={(key) => key}
        onOpenFor={onOpenFor}
        actions={(item) => (
          <ContextMenuItem icon={Check} label={`Act on ${item()}`} onSelect={() => {}} />
        )}
      >
        <div data-item="main">main</div>
      </RowMenu>
    ));

    await user.pointer({ keys: "[MouseRight]", target: screen.getByText("main") });
    expect(await screen.findByRole("menuitem", { name: "Act on main" })).toBeVisible();
    expect(onOpenFor).toHaveBeenLastCalledWith("main");

    setRepositoryId("b");
    flush();
    await vi.waitFor(() => expect(screen.queryByRole("menuitem")).not.toBeInTheDocument());
    expect(onOpenFor).toHaveBeenLastCalledWith(undefined);

    setRepositoryId("a");
    flush();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
  });
});
