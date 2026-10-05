import type { Ref, Uncommitted } from "@gitto/git/types";
import { fireEvent, render, screen } from "@solidjs/testing-library";
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
    branch: { switch: vi.fn(), create: vi.fn() },
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

/**
 * The sidebar's list and the toolbar, which shows a switch or a branch being created running, and
 * why it failed.
 */
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

/** What's shown running: the current branch, in the toolbar, while switching. */
const busy = () => document.querySelector("[aria-busy=true]");

/** Right-clicks the branch `name`'s row, and chooses Create branch in its menu. */
async function createFrom(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.pointer({ keys: "[MouseRight]", target: await row(name) });
  await user.click(await screen.findByRole("menuitem", { name: "Create branch…" }));
}

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

describe("the branch menu", () => {
  it("creates a branch from the one right-clicked, and switches to it, as a switch", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.branch.create.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await createFrom(user, "feature");
    expect(
      await screen.findByText(
        "From feature, with your uncommitted changes, unless they conflict with it: then they're kept in the stash.",
      ),
    ).toBeInTheDocument();
    const name = screen.getByRole("textbox", { name: "Name" });
    await vi.waitFor(() => expect(name).toHaveFocus());
    expect(screen.getByRole("button", { name: "Create branch" })).toBeDisabled();

    await user.keyboard("  topic  {Enter}");
    expect(rpc.git.branch.create).toHaveBeenCalledWith({
      repositoryId: "a",
      name: "topic",
      from: "refs/heads/feature",
    });
    // The dialog closes, and the current branch shows it running.
    await vi.waitFor(() => expect(screen.queryByText("New branch")).not.toBeInTheDocument());
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());

    // Nor another switch meanwhile.
    await user.pointer({ keys: "[MouseRight]", target: await row("main") });
    expect(await screen.findByRole("menuitem", { name: "Create branch…" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    await user.dblClick(await row("origin/feature"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();

    finish();
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
  });

  it("doesn't create one while switching", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.branch.switch.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    // The dialog was open before the switch began.
    await createFrom(user, "main");
    await user.keyboard("topic");
    // Dispatched straight to the row: the dialog keeps the sidebar from being used, so this stands
    // for a switch begun some other way.
    fireEvent.dblClick(await row("feature"));
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());
    expect(await screen.findByRole("button", { name: "Create branch" })).toBeDisabled();
    await user.keyboard("{Enter}");
    expect(rpc.git.branch.create).not.toHaveBeenCalled();

    finish();
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: "Create branch" })).toBeEnabled(),
    );
  });

  it("says the changes come along from the current branch", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await createFrom(user, "main");
    expect(
      await screen.findByText("From main, with your uncommitted changes."),
    ).toBeInTheDocument();
  });

  it("creates one from a remote branch", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.branch.create.mockResolvedValue(undefined);

    await createFrom(user, "origin/feature");
    await user.type(await screen.findByRole("textbox", { name: "Name" }), "topic{Enter}");
    expect(rpc.git.branch.create).toHaveBeenCalledWith({
      repositoryId: "a",
      name: "topic",
      from: "refs/remotes/origin/feature",
    });
  });

  it("opens nothing for what isn't a branch", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await user.pointer({ keys: "[MouseRight]", target: await row("origin") });
    await user.pointer({ keys: "[MouseRight]", target: await row("feature") });
    expect(await screen.findByRole("menuitem", { name: "Create branch…" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await vi.waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());

    await user.pointer({ keys: "[MouseRight]", target: await row("origin") });
    // oxlint-disable-next-line no-promise-executor-return -- a moment for it to open, if it did.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("says why a branch couldn't be created, and keeps the name to fix", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "fatal: a branch named 'topic' already exists";
    rpc.git.branch.create.mockRejectedValue(new Error(message));

    await createFrom(user, "feature");
    await user.type(await screen.findByRole("textbox", { name: "Name" }), "topic{Enter}");
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Switch branch")).toBeInTheDocument();

    await createFrom(user, "feature");
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("topic");
    // Out of the way of the name.
    expect(screen.queryByText(message)).not.toBeInTheDocument();
  });
});
