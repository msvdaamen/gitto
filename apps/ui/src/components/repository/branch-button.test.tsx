import type { Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
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
  changes: { staged: [], unstaged: [], uncounted: false },
  version: "v1",
};

const button = () => screen.getByRole("button", { name: "Branch" });

/** The Branch button, once it has `title`: until the status loads it's a placeholder. */
async function loadedButton(title: string) {
  await vi.waitFor(() => expect(button()).toHaveAttribute("title", title));
  return button();
}

/** Renders the button for the repository `repositoryId()`, "a" to start with. */
function renderButton() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  const [repositoryId, setRepositoryId] = createSignal("a");
  render(() => (
    <QueryClientProvider client={client}>
      <BranchButton repositoryId={repositoryId()} />
    </QueryClientProvider>
  ));
  return setRepositoryId;
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
    expect(screen.getByRole("button", { name: "Create branch" })).toBeDisabled();

    let finish!: () => void;
    rpc.git.branch.create.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));
    // The name field has focus once the popover opens.
    await user.keyboard("  feature/new  {Enter}");
    expect(rpc.git.branch.create).toHaveBeenCalledWith({ repositoryId: "a", name: "feature/new" });
    // The popover closes, and the button shows it running.
    await vi.waitFor(() => expect(screen.queryByText("New branch")).not.toBeInTheDocument());
    expect(button()).toHaveAttribute("aria-busy", "true");
    expect(button()).toBeDisabled();

    rpc.git.status.get.mockResolvedValue({
      ...status,
      head: { ...status.head, name: "feature/new" },
      version: "v2",
    });
    finish();
    expect(await loadedButton("Create a branch from feature/new")).toBeEnabled();
    expect(button()).not.toHaveAttribute("aria-busy", "true");

    // The next branch gets a fresh name.
    await user.click(button());
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("");
  });

  it("says why a branch couldn't be created, and keeps the name to fix", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.branch.create.mockRejectedValue(new Error("fatal: cannot switch branch while merging"));
    renderButton();

    await user.click(await loadedButton("Create a branch from main"));
    await user.keyboard("topic");
    await user.click(await screen.findByRole("button", { name: "Create branch" }));
    expect(
      await screen.findByText("fatal: cannot switch branch while merging"),
    ).toBeInTheDocument();

    await user.click(button());
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("topic");
    // Out of the way of the name.
    expect(screen.queryByText("fatal: cannot switch branch while merging")).not.toBeInTheDocument();
  });

  it("keeps a creation with the repository it's in", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    let fail!: (error: Error) => void;
    rpc.git.branch.create.mockReturnValue(new Promise<void>((_, reject) => (fail = reject)));
    const setRepositoryId = renderButton();

    await user.click(await loadedButton("Create a branch from main"));
    await user.keyboard("topic{Enter}");
    await vi.waitFor(() => expect(button()).toHaveAttribute("aria-busy", "true"));

    // Another repository's button can be used meanwhile, and opening it changes nothing in "a".
    setRepositoryId("b");
    await vi.waitFor(() => expect(button()).toBeEnabled());
    await user.click(button());
    expect(await screen.findByText("New branch")).toBeInTheDocument();
    await user.keyboard("{Escape}");

    setRepositoryId("a");
    await vi.waitFor(() => expect(button()).toHaveAttribute("aria-busy", "true"));
    fail(new Error("fatal: a branch named 'topic' already exists"));
    expect(
      await screen.findByText("fatal: a branch named 'topic' already exists"),
    ).toBeInTheDocument();
  });

  it("names the commit it's made from with HEAD detached", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue({
      ...status,
      head: { kind: "detached", sha: "abc" },
      counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    });
    renderButton();

    await user.click(await loadedButton("Create a branch from this commit"));
    expect(await screen.findByText("From this commit.")).toBeInTheDocument();
  });
});
