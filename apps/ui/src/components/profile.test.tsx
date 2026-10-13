import { render, screen } from "@solidjs/testing-library";
import type * as SolidRouter from "@tanstack/solid-router";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Profile } from "./profile";

const navigate = vi.hoisted(() => vi.fn());

vi.mock("@tanstack/solid-router", async (importOriginal) => ({
  ...(await importOriginal<typeof SolidRouter>()),
  useNavigate: () => navigate,
}));

afterEach(() => {
  vi.resetAllMocks();
});

describe("the profile menu", () => {
  it("opens the settings", async () => {
    const user = userEvent.setup();
    render(() => <Profile />);

    await user.click(screen.getByRole("button", { name: "Open profile" }));
    await user.click(await screen.findByRole("menuitem", { name: "Settings" }));

    expect(navigate).toHaveBeenCalledWith({ to: "/settings" });
  });
});
