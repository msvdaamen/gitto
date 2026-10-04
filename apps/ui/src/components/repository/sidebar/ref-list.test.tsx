import type { Ref, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { RepositoryToolbar } from "../toolbar";
import { RefList } from "./ref-list";

const rpc = vi.hoisted(() => ({
  repository: { list: async () => [] },
  git: {
    status: { get: vi.fn() },
    refs: { list: vi.fn() },
    stash: { list: vi.fn() },
    branch: { switch: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 1, staged: 0, unstaged: 1, conflicted: 0 },
  changes: { staged: [], unstaged: [], uncounted: false },
  version: "v1",
};

function branch(name: string, current = false, kind: "local" | "remote" = "local"): Ref {
  return {
    name,
    fullName: `refs/${kind === "local" ? "heads" : "remotes"}/${name}`,
    kind,
    sha: "abc",
    current,
    upstream: null,
    ahead: 0,
    behind: 0,
  };
}

/** The sidebar's list and the toolbar, which shows a switch running and why it failed. */
function renderSidebar() {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <RepositoryToolbar
        repositoryId="a"
        search=""
        onSearch={() => undefined}
        sidebarOpen
        onToggleSidebar={() => undefined}
        detailsOpen
        onToggleDetails={() => undefined}
      />
      <RefList repositoryId="a" />
    </QueryClientProvider>
  ));
}

/** The sidebar's row for the branch `name`, e.g. `origin/main`. */
const row = (name: string) => screen.findByTitle(name);

beforeEach(() => {
  // jsdom has no layout: give the sections' lists room for their rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(300);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(200);
  rpc.git.status.get.mockResolvedValue(status);
  rpc.git.refs.list.mockResolvedValue([
    branch("main", true),
    branch("feature"),
    branch("origin/feature", false, "remote"),
  ]);
  rpc.git.stash.list.mockResolvedValue([]);
});

afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("switching branches from the sidebar", () => {
  it("switches to a branch that's double-clicked, showing it running", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.branch.switch.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await user.click(await row("feature"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();
    await user.dblClick(await row("feature"));
    expect(rpc.git.branch.switch).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/heads/feature",
    });
    await vi.waitFor(() => expect(document.querySelector("[aria-busy=true]")).toBeInTheDocument());

    finish();
    await vi.waitFor(() =>
      expect(document.querySelector("[aria-busy=true]")).not.toBeInTheDocument(),
    );
  });

  it("switches to a remote branch that's double-clicked", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.branch.switch.mockResolvedValue(undefined);

    await user.dblClick(await row("origin/feature"));
    expect(rpc.git.branch.switch).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/remotes/origin/feature",
    });
  });

  it("doesn't switch to the current branch", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await user.dblClick(await row("main"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();
  });

  it("says when the changes were kept in the stash", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message =
      "Switched to feature, but your uncommitted changes conflict with it, so they're kept in the stash.";
    rpc.git.branch.switch.mockRejectedValue(new Error(message));

    await user.dblClick(await row("feature"));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Switch branch")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    await vi.waitFor(() => expect(screen.queryByText(message)).not.toBeInTheDocument());
  });
});
