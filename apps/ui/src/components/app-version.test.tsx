import type { UpdateState } from "@gitto/system/types";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AppVersion } from "./app-version";

const rpc = vi.hoisted(() => ({
  system: {
    update: { watch: vi.fn(), install: vi.fn() },
    changelog: { all: vi.fn(), unseen: vi.fn(), seen: vi.fn() },
  },
}));

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
    renderWith({ version: "1.2.4-nightly29853462", channel: "nightly", update: null });

    expect(await screen.findByText("Gitto Nightly")).toBeInTheDocument();
    expect(screen.getByText("v1.2.4-nightly29853462")).toBeInTheDocument();
  });

  it("says an update's downloading", async () => {
    renderWith({
      version: "1.2.3",
      channel: "release",
      update: { version: "1.2.4", ready: false },
    });

    expect(await screen.findByText("Downloading update…")).toBeInTheDocument();
    expect(screen.getByText("Gitto")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Restart/ })).not.toBeInTheDocument();
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

  it("opens what's new in every version", async () => {
    const user = userEvent.setup();
    rpc.system.changelog.all.mockResolvedValue([]);
    renderWith({ version: "1.2.3", channel: "release", update: null });

    const version = await screen.findByRole("button", { name: /^Gitto\s*v1\.2\.3$/ });
    expect(version).toHaveAttribute("title", "What's new");
    await user.click(version);
    expect(rpc.system.changelog.all).toHaveBeenCalled();
  });
});
