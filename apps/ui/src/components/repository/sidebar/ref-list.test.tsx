import type { ChangedFile, Ref, Stash, Uncommitted } from "@gitto/git/types";
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
    branch: { switch: vi.fn(), create: vi.fn(), merge: vi.fn() },
    operation: { get: vi.fn() },
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
function renderSidebar(
  repositoryId: () => string = () => "a",
  options: {
    onResolve?: (file: ChangedFile) => void;
    beforeChange?: () => Promise<boolean>;
  } = {},
) {
  const client = new QueryClient({ defaultOptions: queryClient.getDefaultOptions() });
  render(() => (
    <QueryClientProvider client={client}>
      <BranchMenuProvider
        repositoryId={repositoryId()}
        onResolve={options.onResolve}
        beforeChange={options.beforeChange}
      >
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
  rpc.git.operation.get.mockResolvedValue(null);
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

/** Right-clicks the branch `name`'s row, and finds its menu's item `label`. */
async function menuItem(user: ReturnType<typeof userEvent.setup>, name: string, label: string) {
  await user.pointer({ keys: "[MouseRight]", target: await row(name) });
  return screen.findByRole("menuitem", { name: label });
}

/** Right-clicks the branch `name`'s row, and chooses to merge it into `main` in its menu. */
async function mergeFrom(user: ReturnType<typeof userEvent.setup>, name: string) {
  const item = await menuItem(user, name, `Merge ${name} into main`);
  // Enabled once the operation under way, if any, has loaded.
  await vi.waitFor(() => expect(item).toHaveAttribute("aria-disabled", "false"));
  await user.click(item);
}

/** A conflicted file, as the status lists it. */
function conflicted(path: string): ChangedFile {
  return { path, status: "conflicted", origPath: null, additions: 0, deletions: 0 };
}

describe("merging from the branch menu", () => {
  it("merges the branch right-clicked into the checked-out one, showing it running", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: (outcome: string) => void;
    rpc.git.branch.merge.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await mergeFrom(user, "feature");
    expect(rpc.git.branch.merge).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/heads/feature",
      into: "main",
    });
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());

    // Nor another merge, or a switch, meanwhile.
    expect(
      await menuItem(user, "origin/feature", "Merge origin/feature into main"),
    ).toHaveAttribute("aria-disabled", "true");
    await user.keyboard("{Escape}");

    const statusCalls = rpc.git.status.get.mock.calls.length;
    finish("merged");
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
    // The repository was reloaded.
    expect(rpc.git.status.get.mock.calls.length).toBeGreaterThan(statusCalls);
    expect(screen.queryByText("Merge")).not.toBeInTheDocument();
  });

  it("merges a remote branch", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.branch.merge.mockResolvedValue("fast-forward");

    await mergeFrom(user, "origin/feature");
    expect(rpc.git.branch.merge).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/remotes/origin/feature",
      into: "main",
    });
  });

  it("isn't offered for the checked-out branch", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await menuItem(user, "main", "Create branch…");
    expect(screen.queryByRole("menuitem", { name: /^Merge/ })).not.toBeInTheDocument();
  });

  it("says when there was nothing to merge", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.branch.merge.mockResolvedValue("up-to-date");

    await mergeFrom(user, "feature");
    expect(
      await screen.findByText("Already up to date: main has every commit of feature."),
    ).toBeInTheDocument();
    expect(screen.getByText("Merge")).toBeInTheDocument();
  });

  it("doesn't reload the repository when there was nothing to merge", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.branch.merge.mockResolvedValue("up-to-date");
    await vi.waitFor(() => expect(rpc.git.status.get).toHaveBeenCalled());

    const statusCalls = rpc.git.status.get.mock.calls.length;
    const refsCalls = rpc.git.refs.list.mock.calls.length;
    await mergeFrom(user, "feature");
    await screen.findByText("Already up to date: main has every commit of feature.");
    expect(rpc.git.status.get).toHaveBeenCalledTimes(statusCalls);
    expect(rpc.git.refs.list).toHaveBeenCalledTimes(refsCalls);
  });

  it("doesn't switch branches, or create one, while it runs", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: (outcome: string) => void;
    rpc.git.branch.merge.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await mergeFrom(user, "feature");
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());
    await user.dblClick(await row("origin/feature"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();
    expect(await menuItem(user, "feature", "Create branch…")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    // Nor at HEAD, from the toolbar, which switches to it too.
    await vi.waitFor(() => expect(screen.getByRole("button", { name: "Branch" })).toBeDisabled());

    finish("merged");
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
    await user.dblClick(await row("origin/feature"));
    expect(rpc.git.branch.switch).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/remotes/origin/feature",
    });
  });

  it("doesn't create a branch from the dialog that was open as it began", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: (outcome: string) => void;
    rpc.git.branch.merge.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await createFrom(user, "main");
    await user.keyboard("topic");
    // Dispatched straight to the row and the menu: the dialog keeps the sidebar from being used, so
    // this stands for a merge begun some other way.
    fireEvent.contextMenu(await row("feature"));
    const merge = await screen.findByRole("menuitem", { name: "Merge feature into main" });
    await vi.waitFor(() => expect(merge).toHaveAttribute("aria-disabled", "false"));
    fireEvent.keyDown(merge, { key: "Enter" });
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());
    const create = await screen.findByRole("button", { name: "Create branch", hidden: true });
    expect(create).toBeDisabled();
    fireEvent.submit(create.closest("form")!);
    expect(rpc.git.branch.create).not.toHaveBeenCalled();

    finish("merged");
    await vi.waitFor(() => expect(create).toBeEnabled());
  });

  it("shows the failure that came last, a switch's after a merge's", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.branch.merge.mockRejectedValue(new Error("The merge failed."));
    rpc.git.branch.switch.mockRejectedValue(new Error("The switch failed."));

    await mergeFrom(user, "feature");
    expect(await screen.findByText("The merge failed.")).toBeInTheDocument();
    // Dispatched straight to the row, as a switch from the keyboard would leave the popover open.
    fireEvent.dblClick(await row("origin/feature"));
    expect(await screen.findByText("The switch failed.")).toBeInTheDocument();
    expect(screen.getByText("Switch branch")).toBeInTheDocument();

    // The merge's is still there once that's dismissed.
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(await screen.findByText("The merge failed.")).toBeInTheDocument();
  });

  it("says why a merge failed, until that's dismissed", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message =
      "Merging feature would overwrite your uncommitted changes to a.txt. Commit or stash them, then merge.";
    rpc.git.branch.merge.mockRejectedValue(new Error(message));

    await mergeFrom(user, "feature");
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Merge")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    await vi.waitFor(() => expect(screen.queryByText(message)).not.toBeInTheDocument());
  });

  it("opens the first conflicted file with markers left once it stops at conflicts", async () => {
    const user = userEvent.setup();
    const onResolve = vi.fn();
    renderSidebar(undefined, { onResolve });
    rpc.git.branch.merge.mockImplementation(async () => {
      // As the merge left it, which the repository reloads.
      rpc.git.status.get.mockResolvedValue({
        ...status,
        counts: { files: 2, staged: 0, unstaged: 2, conflicted: 2 },
        changes: {
          staged: [],
          unstaged: [conflicted("a.txt"), conflicted("b.txt")],
          uncounted: false,
          markerFree: ["a.txt"],
        },
        version: "v2",
      });
      return "conflicts";
    });

    await mergeFrom(user, "feature");
    await vi.waitFor(() => expect(onResolve).toHaveBeenCalledWith(conflicted("b.txt")));
    expect(onResolve).toHaveBeenCalledTimes(1);
    // Stopping at conflicts isn't a failure.
    expect(screen.queryByText("Merge")).not.toBeInTheDocument();
  });

  it("opens nothing once it went through", async () => {
    const user = userEvent.setup();
    const onResolve = vi.fn();
    renderSidebar(undefined, { onResolve });
    rpc.git.branch.merge.mockResolvedValue("merged");

    await mergeFrom(user, "feature");
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
    expect(onResolve).not.toHaveBeenCalled();
  });

  it("opens nothing in the repository switched to while it ran", async () => {
    const user = userEvent.setup();
    const onResolve = vi.fn();
    const [repositoryId, setRepositoryId] = createSignal("a");
    renderSidebar(repositoryId, { onResolve });
    let finish!: (outcome: string) => void;
    rpc.git.branch.merge.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await mergeFrom(user, "feature");
    setRepositoryId("b");
    rpc.git.status.get.mockResolvedValue({
      ...status,
      counts: { files: 1, staged: 0, unstaged: 1, conflicted: 1 },
      changes: { staged: [], unstaged: [conflicted("a.txt")], uncounted: false, markerFree: [] },
    });
    finish("conflicts");
    // oxlint-disable-next-line no-promise-executor-return -- a moment for it to open one, if it did.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(onResolve).not.toHaveBeenCalled();
  });

  it("waits for the edits to a file on show to be saved, and doesn't merge if they aren't", async () => {
    const user = userEvent.setup();
    let saved = false;
    const beforeChange = vi.fn(async () => saved);
    renderSidebar(undefined, { beforeChange });
    rpc.git.branch.merge.mockResolvedValue("merged");

    await mergeFrom(user, "feature");
    await vi.waitFor(() => expect(beforeChange).toHaveBeenCalledTimes(1));
    expect(rpc.git.branch.merge).not.toHaveBeenCalled();

    saved = true;
    await mergeFrom(user, "feature");
    await vi.waitFor(() => expect(rpc.git.branch.merge).toHaveBeenCalledTimes(1));
  });

  it("isn't offered while conflicts are left to resolve", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue({
      ...status,
      counts: { files: 1, staged: 0, unstaged: 1, conflicted: 1 },
    });
    renderSidebar();

    const item = await menuItem(user, "feature", "Merge feature into main");
    // oxlint-disable-next-line no-promise-executor-return -- a moment for the operation to load.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(item).toHaveAttribute("aria-disabled", "true");
  });

  it("isn't offered while a merge or a rebase is under way", async () => {
    const user = userEvent.setup();
    rpc.git.operation.get.mockResolvedValue({ kind: "merge", merging: "other", into: "main" });
    renderSidebar();

    const item = await menuItem(user, "feature", "Merge feature into main");
    await vi.waitFor(() => expect(rpc.git.operation.get).toHaveBeenCalled());
    // oxlint-disable-next-line no-promise-executor-return -- a moment for the operation to load.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(item).toHaveAttribute("aria-disabled", "true");
    await user.click(item);
    expect(rpc.git.branch.merge).not.toHaveBeenCalled();
  });

  it("isn't offered with HEAD detached", async () => {
    const user = userEvent.setup();
    rpc.git.status.get.mockResolvedValue({ ...status, head: { kind: "detached", sha: "abc" } });
    renderSidebar();

    const item = await menuItem(user, "feature", "Merge feature into detached HEAD");
    // oxlint-disable-next-line no-promise-executor-return -- a moment for the operation to load.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(item).toHaveAttribute("aria-disabled", "true");
  });

  it("isn't offered while switching branches", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.branch.switch.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await user.dblClick(await row("feature"));
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());
    expect(
      await menuItem(user, "origin/feature", "Merge origin/feature into main"),
    ).toHaveAttribute("aria-disabled", "true");
    await user.keyboard("{Escape}");

    finish();
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
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
