import type { Stash, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { StashButtons } from "./stash-buttons";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn() },
    stash: { list: vi.fn(), push: vi.fn(), pop: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 2, staged: 1, unstaged: 1, conflicted: 0 },
  changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
  version: "v1",
};
const clean: Uncommitted = {
  ...status,
  counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
  version: "v2",
};
const newer: Stash = {
  sha: "b".repeat(40),
  base: "abc",
  message: "WIP on main: abc newer",
  createdAt: 2,
};
const older: Stash = { sha: "a".repeat(40), base: "abc", message: "On main: older", createdAt: 1 };

/** The button labelled `name`, once it has `title`: until the status loads it's a placeholder. */
async function loadedButton(name: "Stash" | "Pop", title: string) {
  await vi.waitFor(() =>
    expect(screen.getByRole("button", { name })).toHaveAttribute("title", title),
  );
  return screen.getByRole("button", { name });
}

function renderButtons() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <StashButtons repositoryId="repo" />
    </QueryClientProvider>
  ));
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the stash buttons", () => {
  it("stash the changes, and pop the newest stash back", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.stash.list.mockResolvedValue([older]);
    renderButtons();

    expect(await loadedButton("Pop", 'Pop "On main: older"')).toHaveTextContent("1");
    const stash = await loadedButton("Stash", "Stash 2 changed files");

    let finishStash!: () => void;
    rpc.git.stash.push.mockReturnValue(new Promise<void>((resolve) => (finishStash = resolve)));
    await user.click(stash);
    expect(rpc.git.stash.push).toHaveBeenCalledWith({ repositoryId: "repo" });
    expect(stash).toHaveAttribute("aria-busy", "true");
    // A pop would name the stash that's being replaced as the newest.
    expect(screen.getByRole("button", { name: "Pop" })).toBeDisabled();

    rpc.git.status.get.mockResolvedValue(clean);
    rpc.git.stash.list.mockResolvedValue([newer, older]);
    finishStash();
    const pop = await loadedButton("Pop", 'Pop "WIP on main: abc newer"');
    expect(pop).toHaveTextContent("2");
    expect(await loadedButton("Stash", "No changes to stash.")).toBeDisabled();

    rpc.git.stash.pop.mockResolvedValue(undefined);
    await user.click(pop);
    expect(rpc.git.stash.pop).toHaveBeenCalledWith({ repositoryId: "repo", sha: newer.sha });
  });

  it("say why a pop failed", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue(status);
    rpc.git.stash.list.mockResolvedValue([older]);
    rpc.git.stash.pop.mockRejectedValue(new Error("Popping the stash caused conflicts."));
    renderButtons();

    await user.click(await loadedButton("Pop", 'Pop "On main: older"'));
    expect(await screen.findByText("Popping the stash caused conflicts.")).toBeInTheDocument();
    // The repository is reloaded, as the pop may have left conflicts.
    expect(rpc.git.stash.list).toHaveBeenCalledTimes(2);

    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    await vi.waitFor(() =>
      expect(screen.queryByText("Popping the stash caused conflicts.")).not.toBeInTheDocument(),
    );
  });

  it("are disabled with nothing to stash or pop", async () => {
    rpc.git.status.get.mockResolvedValue(clean);
    rpc.git.stash.list.mockResolvedValue([]);
    renderButtons();

    expect(await loadedButton("Stash", "No changes to stash.")).toBeDisabled();
    const pop = await loadedButton("Pop", "No stashes to pop.");
    expect(pop).toBeDisabled();
    expect(pop).not.toHaveTextContent("0");
  });
});
