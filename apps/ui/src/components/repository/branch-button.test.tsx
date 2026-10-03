import type { Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { BranchButton } from "./branch-button";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn() },
    branch: { create: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 2, staged: 1, unstaged: 1, conflicted: 0 },
  changes: { staged: [], unstaged: [] },
  version: "v1",
};

/** The Branch button, once it has `title`: until the status loads it's a placeholder. */
async function loadedButton(title: string) {
  await vi.waitFor(() =>
    expect(screen.getByRole("button", { name: "Branch" })).toHaveAttribute("title", title),
  );
  return screen.getByRole("button", { name: "Branch" });
}

function renderButton() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <BranchButton repositoryId="repo" />
    </QueryClientProvider>
  ));
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the branch button", () => {
  it("creates a branch from the current one, with the changes", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    renderButton();

    await user.click(await loadedButton("Create a branch from main"));
    expect(
      await screen.findByText("From main, with your uncommitted changes."),
    ).toBeInTheDocument();
    const create = screen.getByRole("button", { name: "Create branch" });
    expect(create).toBeDisabled();

    let finish!: () => void;
    rpc.git.branch.create.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));
    // The name field has focus once the popover opens.
    await user.keyboard("  feature/new  {Enter}");
    expect(rpc.git.branch.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      name: "feature/new",
    });
    expect(screen.getByRole("button", { name: "Branch" })).toHaveAttribute("aria-busy", "true");

    rpc.git.status.get.mockResolvedValue({
      ...status,
      head: { ...status.head, name: "feature/new" },
      version: "v2",
    });
    finish();
    await loadedButton("Create a branch from feature/new");
    await vi.waitFor(() => expect(screen.queryByText("New branch")).not.toBeInTheDocument());
  });

  it("says why a branch couldn't be created, and keeps the name", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.branch.create.mockRejectedValue(
      new Error("fatal: a branch named 'main' already exists"),
    );
    renderButton();

    await user.click(await loadedButton("Create a branch from main"));
    await user.keyboard("main");
    await user.click(await screen.findByRole("button", { name: "Create branch" }));
    expect(
      await screen.findByText("fatal: a branch named 'main' already exists"),
    ).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("main");
  });

  it("is disabled with conflicts", async () => {
    rpc.git.status.get.mockResolvedValue({
      ...status,
      counts: { ...status.counts, conflicted: 1 },
    });
    renderButton();

    expect(await loadedButton("Resolve the conflicts before creating a branch.")).toBeDisabled();
  });
});
