import type { Ref, Stash, Uncommitted } from "@gitto/git/types";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { BranchMenuProvider } from "../branch-menu";
import { RepositoryToolbar } from "../toolbar";
import { RefList } from "./ref-list";

const rpc = vi.hoisted(() => ({
  repository: { list: async () => [] },
  git: {
    status: { get: vi.fn() },
    refs: { list: vi.fn() },
    stash: { list: vi.fn(), pop: vi.fn(), drop: vi.fn() },
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
  changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
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
function renderSidebar(repositoryId: () => string = () => "a") {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <BranchMenuProvider repositoryId={repositoryId()}>
        <RepositoryToolbar
          repositoryId={repositoryId()}
          search=""
          onSearch={() => undefined}
          sidebarOpen
          onToggleSidebar={() => undefined}
          detailsOpen
          onToggleDetails={() => undefined}
        />
        <RefList repositoryId={repositoryId()} />
      </BranchMenuProvider>
    </QueryClientProvider>
  ));
}

const newer: Stash = { sha: "b".repeat(40), base: "abc", message: "On main: newer", createdAt: 2 };
const older: Stash = { sha: "a".repeat(40), base: "abc", message: "On main: older", createdAt: 1 };

/** The sidebar's row for the branch `name`, e.g. `origin/main`. */
const row = (name: string) => screen.findByTitle(name);

/** What's shown running: the current branch, in the toolbar, while switching. */
const busy = () => document.querySelector("[aria-busy=true]");

/** The sidebar section shown running something on its items, e.g. popping a stash. */
const busySection = () => document.querySelector("section[aria-busy=true]");

/** Right-clicks the branch `name`'s row, and chooses Create branch in its menu. */
async function createFrom(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.pointer({ keys: "[MouseRight]", target: await row(name) });
  await user.click(await screen.findByRole("menuitem", { name: "Create branch…" }));
}

/** Expands the stashes, and right-clicks the row of the stash with `message`. */
async function openMenu(user: ReturnType<typeof userEvent.setup>, message: string) {
  if (!screen.queryByTitle(new RegExp(`^${message} Stashed`))) {
    // The section's header, rather than its icon in the narrow sidebar.
    await user.click(await screen.findByRole("button", { name: /^Stashes\s*\d+$/ }));
  }
  const stashRow = await screen.findByTitle(new RegExp(`^${message} Stashed`));
  await user.pointer({ keys: "[MouseRight]", target: stashRow });
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

describe("the stash menu", () => {
  beforeEach(() => {
    rpc.git.stash.list.mockResolvedValue([newer, older]);
  });

  it("pops the stash right-clicked, an older one too, showing it running on the section", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.stash.pop.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await openMenu(user, "On main: older");
    await user.click(await screen.findByRole("menuitem", { name: "Pop stash" }));
    expect(rpc.git.stash.pop).toHaveBeenCalledWith({ repositoryId: "a", sha: older.sha });
    await vi.waitFor(() => expect(busySection()).toHaveTextContent("Stashes"));
    // The toolbar's Pop isn't the one running, but waits for it. Found once the menu has closed,
    // which hides the rest while it's open.
    const pop = await screen.findByRole("button", { name: "Pop" });
    expect(pop).not.toHaveAttribute("aria-busy", "true");
    expect(pop).toBeDisabled();

    // Nor another one meanwhile.
    await openMenu(user, "On main: newer");
    expect(await screen.findByRole("menuitem", { name: "Pop stash" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(screen.getByRole("menuitem", { name: "Delete stash…" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");

    finish();
    await vi.waitFor(() => expect(busySection()).not.toBeInTheDocument());
  });

  it("says beside the section why a stash couldn't be popped", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "Popping the stash caused conflicts.";
    rpc.git.stash.pop.mockRejectedValue(new Error(message));

    await openMenu(user, "On main: older");
    await user.click(await screen.findByRole("menuitem", { name: "Pop stash" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Pop stash")).toBeInTheDocument();
    // Only there, not under the toolbar's Pop as well.
    expect(screen.getAllByText(message)).toHaveLength(1);
  });

  it("doesn't delete one while another stash action runs", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.stash.pop.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    // The dialog was open before the pop began.
    await openMenu(user, "On main: older");
    await user.click(await screen.findByRole("menuitem", { name: "Delete stash…" }));
    // Dispatched straight to the button: the dialog keeps the toolbar from being used, so this
    // stands for a pop begun some other way.
    const pop = screen.getByRole("button", { name: "Pop", hidden: true });
    await vi.waitFor(() => expect(pop).toBeEnabled());
    fireEvent.click(pop);
    await vi.waitFor(() =>
      expect(rpc.git.stash.pop).toHaveBeenCalledWith({ repositoryId: "a", sha: newer.sha }),
    );
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    expect(rpc.git.stash.drop).not.toHaveBeenCalled();

    finish();
  });

  it("forgets the stash it was asking about deleting in another repository", async () => {
    const user = userEvent.setup();
    const [repositoryId, setRepositoryId] = createSignal("a");
    renderSidebar(repositoryId);

    await openMenu(user, "On main: older");
    await user.click(await screen.findByRole("menuitem", { name: "Delete stash…" }));
    expect(await screen.findByRole("alertdialog")).toBeInTheDocument();

    setRepositoryId("b");
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(rpc.git.stash.drop).not.toHaveBeenCalled();
  });

  it("doesn't pop one over conflicts", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue({
      ...status,
      counts: { files: 1, staged: 0, unstaged: 0, conflicted: 1 },
    });
    renderSidebar();

    await openMenu(user, "On main: newer");
    expect(await screen.findByRole("menuitem", { name: "Pop stash" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("deletes the stash right-clicked once that's confirmed", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.stash.drop.mockResolvedValue(undefined);

    await openMenu(user, "On main: older");
    await user.click(await screen.findByRole("menuitem", { name: "Delete stash…" }));
    expect(await screen.findByText(/"On main: older" is deleted/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(rpc.git.stash.drop).not.toHaveBeenCalled();

    await openMenu(user, "On main: older");
    await user.click(await screen.findByRole("menuitem", { name: "Delete stash…" }));
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    expect(rpc.git.stash.drop).toHaveBeenCalledWith({ repositoryId: "a", sha: older.sha });
    expect(rpc.git.stash.pop).not.toHaveBeenCalled();
  });

  it("says why a stash couldn't be deleted", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "The stash is gone: the stashes changed before it could be deleted.";
    rpc.git.stash.drop.mockRejectedValue(new Error(message));

    await openMenu(user, "On main: newer");
    await user.click(await screen.findByRole("menuitem", { name: "Delete stash…" }));
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Delete stash")).toBeInTheDocument();
  });

  it("opens no branch menu for a stash", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await openMenu(user, "On main: newer");
    expect(await screen.findByRole("menuitem", { name: "Pop stash" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Create branch…" })).not.toBeInTheDocument();
  });
});
