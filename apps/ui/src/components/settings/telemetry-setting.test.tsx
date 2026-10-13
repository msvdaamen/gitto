import type { TelemetryState } from "@gitto/system/types";
import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TelemetrySetting } from "./telemetry-setting";

const rpc = vi.hoisted(() => ({
  system: { telemetry: { get: vi.fn(), set: vi.fn() } },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

function renderWith(state: TelemetryState) {
  rpc.system.telemetry.get.mockResolvedValue(state);
  render(() => <TelemetrySetting />);
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the error reports setting", () => {
  it("turns error reports off", async () => {
    const user = userEvent.setup();
    renderWith({ available: true, enabled: true, sending: true });
    rpc.system.telemetry.set.mockResolvedValue({ available: true, enabled: false, sending: false });

    const toggle = await screen.findByRole("switch", { name: "Send error reports" });
    expect(toggle).toBeChecked();
    await user.click(toggle);

    expect(rpc.system.telemetry.set).toHaveBeenCalledWith({ enabled: false });
    expect(toggle).not.toBeChecked();
    expect(screen.queryByText(/when it restarts/)).not.toBeInTheDocument();
  });

  it("says turning them on starts them when Gitto restarts", async () => {
    const user = userEvent.setup();
    renderWith({ available: true, enabled: false, sending: false });
    rpc.system.telemetry.set.mockResolvedValue({ available: true, enabled: true, sending: false });

    await user.click(await screen.findByRole("switch", { name: "Send error reports" }));

    expect(rpc.system.telemetry.set).toHaveBeenCalledWith({ enabled: true });
    expect(
      await screen.findByText("Gitto starts sending them when it restarts."),
    ).toBeInTheDocument();
  });

  it("can't be changed in a build that doesn't send them", async () => {
    renderWith({ available: false, enabled: true, sending: false });

    expect(await screen.findByRole("switch", { name: "Send error reports" })).toBeDisabled();
    expect(screen.getByText("This build of Gitto doesn't send error reports.")).toBeInTheDocument();
  });

  it("says when the setting couldn't be saved", async () => {
    const user = userEvent.setup();
    vi.spyOn(console, "error").mockImplementation(() => {});
    renderWith({ available: true, enabled: true, sending: true });
    rpc.system.telemetry.set.mockRejectedValue(new Error("disk full"));

    const toggle = await screen.findByRole("switch", { name: "Send error reports" });
    await user.click(toggle);

    expect(await screen.findByRole("status")).toHaveTextContent("Couldn't save the setting.");
    expect(toggle).toBeChecked();
  });
});
