import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { GIT_DOWNLOAD_URL, GitVersionDialog } from "./git-version-dialog";

const rpc = vi.hoisted(() => ({ git: { version: { check: vi.fn() } } }));

vi.mock("@/lib/rpc", () => ({ rpc }));

function renderDialog() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <GitVersionDialog />
    </QueryClientProvider>
  ));
  return client;
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the git version dialog", () => {
  it("isn't shown with a supported git", async () => {
    rpc.git.version.check.mockResolvedValue({
      version: "2.43.0",
      required: "2.40",
      supported: true,
    });
    renderDialog();

    await vi.waitFor(() => expect(rpc.git.version.check).toHaveBeenCalled());
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("says which git is installed and which is needed, until git is updated", async () => {
    const user = userEvent.setup();
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    rpc.git.version.check.mockResolvedValue({
      version: "2.39.3",
      required: "2.40",
      supported: false,
    });
    const client = renderDialog();
    client.setQueryData(["git", "repo", "status"], "failed for want of git");

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Gitto needs a newer version of Git");
    expect(dialog).toHaveTextContent("Git 2.39.3 is installed, but Gitto needs Git 2.40 or newer.");

    // It can't be dismissed: nothing works until git is updated.
    await user.keyboard("{Escape}");
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Download Git" }));
    expect(open).toHaveBeenCalledWith(GIT_DOWNLOAD_URL, "_blank");

    await user.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Still finding Git 2.39.3.");

    rpc.git.version.check.mockResolvedValue({
      version: "2.51.0",
      required: "2.40",
      supported: true,
    });
    await user.click(screen.getByRole("button", { name: "Check again" }));
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    // What failed meanwhile is loaded again.
    expect(client.getQueryState(["git", "repo", "status"])?.isInvalidated).toBe(true);
  });

  it("says when git can't be found", async () => {
    rpc.git.version.check.mockResolvedValue({ version: null, required: "2.40", supported: false });
    renderDialog();

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("Gitto couldn't find Git");
    expect(dialog).toHaveTextContent("Install Git 2.40 or newer to get started.");
  });
});
