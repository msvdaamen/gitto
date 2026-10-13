import type { UpdateState } from "@gitto/system/types";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Notifications } from "./notifications";

const rpc = vi.hoisted(() => ({
  system: { update: { watch: vi.fn(), install: vi.fn() } },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** Renders it with the main process saying `first`, then what `next` says. */
function renderWith(first: UpdateState) {
  const { promise, resolve: next } = Promise.withResolvers<UpdateState>();
  rpc.system.update.watch.mockImplementation(async function* () {
    yield first;
    yield await promise;
  });
  render(() => <Notifications />);
  return { next };
}

const downloading: UpdateState = {
  version: "1.2.3",
  channel: "release",
  update: { version: "1.2.4", ready: false },
};
const ready: UpdateState = { ...downloading, update: { version: "1.2.4", ready: true } };

afterEach(() => {
  vi.resetAllMocks();
});

describe("notifications", () => {
  it("has nothing to tell without an update", async () => {
    const user = userEvent.setup();
    renderWith({ version: "1.2.3", channel: "release", update: null });

    await user.click(await screen.findByRole("button", { name: "Notifications" }));
    expect(await screen.findByText("You're all caught up")).toBeInTheDocument();
  });

  it("tells of a new version until it's seen, and again once it's ready", async () => {
    const user = userEvent.setup();
    const { next } = renderWith(downloading);

    await user.click(await screen.findByRole("button", { name: "Notifications (new)" }));
    expect(await screen.findByText("Gitto 1.2.4 is available")).toBeInTheDocument();
    expect(screen.getByText("Downloading the update…")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();

    next(ready);
    await user.click(await screen.findByRole("button", { name: "Notifications (new)" }));
    await user.click(await screen.findByRole("button", { name: "Restart to update" }));
    expect(rpc.system.update.install).toHaveBeenCalled();
  });
});
