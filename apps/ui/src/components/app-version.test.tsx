import type { UpdateState } from "@gitto/system/types";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppVersion } from "./app-version";

const rpc = vi.hoisted(() => ({ system: { update: { watch: vi.fn(), install: vi.fn() } } }));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** Renders it with the main process saying `states`, in turn. */
function renderWith(...states: UpdateState[]) {
  rpc.system.update.watch.mockImplementation(async function* () {
    yield* states;
  });
  render(() => <AppVersion />);
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the app's version", () => {
  it("names a nightly", async () => {
    renderWith({ version: "1.2.4-nightly.20261005134259", channel: "nightly", update: null });

    expect(await screen.findByText("Gitto Nightly")).toBeInTheDocument();
    expect(screen.getByText("v1.2.4-nightly.20261005134259")).toBeInTheDocument();
  });

  it("says an update's downloading", async () => {
    renderWith({
      version: "1.2.3",
      channel: "release",
      update: { version: "1.2.4", ready: false },
    });

    expect(await screen.findByText("Downloading update…")).toBeInTheDocument();
    expect(screen.getByText("Gitto")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("restarts into an update that's ready", async () => {
    const user = userEvent.setup();
    renderWith(
      { version: "1.2.3", channel: "release", update: { version: "1.2.4", ready: false } },
      { version: "1.2.3", channel: "release", update: { version: "1.2.4", ready: true } },
    );

    await user.click(await screen.findByRole("button", { name: "Restart to update to 1.2.4" }));
    expect(rpc.system.update.install).toHaveBeenCalled();
    expect(screen.queryByText("Downloading update…")).not.toBeInTheDocument();
  });
});
