import type { ChangedFile, Ref, Stash, Uncommitted, Worktree } from "@gitto/git/types";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import type * as SolidRouter from "@tanstack/solid-router";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { queryClient } from "@/lib/query-client";

import { BranchMenuProvider } from "../branch-menu";
import { RepositoryToolbar } from "../toolbar";
import { RefList } from "./ref-list";

const rpc = vi.hoisted(() => ({
  repository: { list: vi.fn(), add: vi.fn(), remove: vi.fn() },
  system: { selectFolder: vi.fn() },
  git: {
    status: { get: vi.fn() },
    refs: { list: vi.fn() },
    stash: { list: vi.fn(), pop: vi.fn(), drop: vi.fn() },
    branch: { switch: vi.fn(), create: vi.fn(), merge: vi.fn(), delete: vi.fn() },
    operation: { get: vi.fn() },
    worktree: { list: vi.fn(), add: vi.fn(), remove: vi.fn() },
    remote: { list: vi.fn(), pull: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** Where the router was sent, e.g. to the worktree opened. */
const navigate = vi.hoisted(() => vi.fn());

vi.mock("@tanstack/solid-router", async (importOriginal) => ({
  ...(await importOriginal<typeof SolidRouter>()),
  useNavigate: () => navigate,
}));

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
  rpc.repository.list.mockResolvedValue([{ id: "a", name: "repo", path: "/w/repo" }]);
  rpc.git.worktree.list.mockResolvedValue([]);
  rpc.git.remote.list.mockResolvedValue(["origin"]);
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

/** `name`, a local branch, tracking `origin/<name>`. */
const tracking = (name: string, current = false): Ref => ({
  ...branch(name, current),
  upstream: `origin/${name}`,
});

describe("pulling from the branch menu", () => {
  beforeEach(() => {
    rpc.git.refs.list.mockResolvedValue([
      tracking("main", true),
      tracking("feature"),
      branch("lonely"),
      branch("origin/feature", false, "remote"),
    ]);
  });

  it("pulls a branch that isn't checked out, showing it running", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.remote.pull.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await user.click(await menuItem(user, "feature", "Pull feature"));
    expect(rpc.git.remote.pull).toHaveBeenCalledWith({ repositoryId: "a", branch: "feature" });
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());

    const refsCalls = rpc.git.refs.list.mock.calls.length;
    finish();
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
    // The repository was reloaded.
    expect(rpc.git.refs.list.mock.calls.length).toBeGreaterThan(refsCalls);
  });

  it("pulls the checked-out branch as the Pull button does", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.remote.pull.mockResolvedValue(undefined);

    await user.click(await menuItem(user, "main", "Pull main"));
    expect(rpc.git.remote.pull).toHaveBeenCalledWith({ repositoryId: "a" });
  });

  it("says why a branch couldn't be pulled", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "feature and origin/feature have diverged. Switch to feature to pull it.";
    rpc.git.remote.pull.mockRejectedValue(new Error(message));

    await user.click(await menuItem(user, "feature", "Pull feature"));
    expect(await screen.findByText(message)).toBeInTheDocument();
  });

  // One that tracks none, a remote one, and one checked out in another worktree.
  it.each(["lonely", "origin/feature", "feature"])("isn't offered for %s", async (name) => {
    const user = userEvent.setup();
    rpc.git.worktree.list.mockResolvedValue(WORKTREES);
    renderSidebar();

    await user.pointer({ keys: "[MouseRight]", target: await branchRow(name) });
    await screen.findByRole("menuitem", { name: "Create branch…" });
    expect(screen.queryByRole("menuitem", { name: /^Pull/ })).not.toBeInTheDocument();
  });

  it("is offered for the branch checked out in the worktree on show", async () => {
    const user = userEvent.setup();
    rpc.git.worktree.list.mockResolvedValue(WORKTREES);
    renderSidebar();

    await user.pointer({ keys: "[MouseRight]", target: await branchRow("main") });
    expect(await screen.findByRole("menuitem", { name: "Pull main" })).toBeInTheDocument();
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

function worktree(path: string, ref: string | null, extra: Partial<Worktree> = {}): Worktree {
  return {
    path,
    name: path.slice(path.lastIndexOf("/") + 1),
    head: "abc",
    branch: ref,
    main: false,
    bare: false,
    current: false,
    locked: null,
    prunable: null,
    ...extra,
  };
}

/**
 * The repository's worktrees: the main one, on show, on `main`; one on `feature`; one detached;
 * and one whose folder is gone.
 */
const WORKTREES = [
  worktree("/w/repo", "refs/heads/main", { main: true, current: true }),
  worktree("/w/repo-feature", "refs/heads/feature"),
  worktree("/w/repo-check", null),
  worktree("/w/repo-gone", null, { prunable: "gitdir file points to non-existent location" }),
];

/** The sidebar's row for the worktree at `path`, whose tooltip starts with it (its lines as one). */
const worktreeRow = (path: string) => screen.findByTitle(new RegExp(`^${path} `));

/** The sidebar's row for the branch `name`, whichever worktree it's checked out in. */
const branchRow = (name: string) =>
  screen.findByTitle((title) => title === name || title.startsWith(`${name} Checked out in`));

/** Right-clicks the row of the worktree at `path`, and finds its menu's item `label`. */
async function worktreeMenuItem(
  user: ReturnType<typeof userEvent.setup>,
  path: string,
  label: string,
) {
  await user.pointer({ keys: "[MouseRight]", target: await worktreeRow(path) });
  return screen.findByRole("menuitem", { name: label });
}

describe("the worktrees", () => {
  beforeEach(() => {
    rpc.git.worktree.list.mockResolvedValue(WORKTREES);
  });

  it("lists them, the one on show marked, and says which branches are checked out in the others", async () => {
    renderSidebar();

    const main = await worktreeRow("/w/repo");
    expect(main).toHaveAttribute("title", "/w/repo\nOn main\nOpen here");
    expect(main).toHaveTextContent("repo");
    expect(await worktreeRow("/w/repo-feature")).toHaveTextContent("repo-featurefeature");
    expect(await worktreeRow("/w/repo-check")).toHaveTextContent("repo-checkdetached");
    expect(screen.getByRole("button", { name: /^Worktrees\s*4$/ })).toBeInTheDocument();

    expect(
      within(await branchRow("feature")).getByRole("img", { name: "Checked out in repo-feature" }),
    ).toBeInTheDocument();
    expect(await branchRow("feature")).toHaveAttribute(
      "title",
      "feature\nChecked out in /w/repo-feature",
    );
    expect(within(await row("main")).queryByRole("img")).not.toBeInTheDocument();
  });

  it("opens a worktree that's double-clicked, as a repository of its own, showing it running", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: (repository: unknown) => void;
    rpc.repository.add.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await user.dblClick(await worktreeRow("/w/repo-feature"));
    expect(rpc.repository.add).toHaveBeenCalledWith({ path: "/w/repo-feature" });
    await vi.waitFor(() => expect(busySection()).toHaveTextContent("Worktrees"));

    finish({ id: "b", name: "repo-feature", path: "/w/repo-feature" });
    await vi.waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: "/$repoId", params: { repoId: "b" } }),
    );
    await vi.waitFor(() => expect(busySection()).not.toBeInTheDocument());
  });

  it("doesn't open the one on show, nor one whose folder is gone", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await user.dblClick(await worktreeRow("/w/repo"));
    await user.dblClick(await worktreeRow("/w/repo-gone"));
    expect(rpc.repository.add).not.toHaveBeenCalled();
    expect(await worktreeMenuItem(user, "/w/repo", "Open worktree")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    expect(await worktreeMenuItem(user, "/w/repo-gone", "Open worktree")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("says beside the section why a worktree couldn't be opened", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "/w/repo-check is not a git repository.";
    rpc.repository.add.mockRejectedValue(new Error(message));

    await user.click(await worktreeMenuItem(user, "/w/repo-check", "Open worktree"));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Open worktree")).toBeInTheDocument();
    expect(navigate).not.toHaveBeenCalled();
  });

  it("removes the worktree right-clicked, asking first, showing it running on the section", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.worktree.remove.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await user.click(await worktreeMenuItem(user, "/w/repo-feature", "Remove worktree…"));
    expect(
      await screen.findByText(
        "The folder /w/repo-feature is deleted, and any uncommitted changes in it are lost. The branch feature is kept.",
      ),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(rpc.git.worktree.remove).toHaveBeenCalledWith({
      repositoryId: "a",
      path: "/w/repo-feature",
    });
    await vi.waitFor(() => expect(busySection()).toHaveTextContent("Worktrees"));
    // Nor another one meanwhile.
    expect(await worktreeMenuItem(user, "/w/repo-check", "Remove worktree…")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");

    const listCalls = rpc.git.worktree.list.mock.calls.length;
    finish();
    await vi.waitFor(() => expect(busySection()).not.toBeInTheDocument());
    // The repository was reloaded.
    expect(rpc.git.worktree.list.mock.calls.length).toBeGreaterThan(listCalls);
  });

  it("removes the repository a worktree was opened as along with it", async () => {
    const user = userEvent.setup();
    rpc.repository.list.mockResolvedValue([
      { id: "a", name: "repo", path: "/w/repo" },
      { id: "b", name: "repo-feature", path: "/w/repo-feature" },
    ]);
    renderSidebar();
    rpc.git.worktree.remove.mockResolvedValue(undefined);
    rpc.repository.remove.mockResolvedValue(undefined);

    await user.click(await worktreeMenuItem(user, "/w/repo-feature", "Remove worktree…"));
    await user.click(await screen.findByRole("button", { name: "Remove" }));
    await vi.waitFor(() => expect(rpc.repository.remove).toHaveBeenCalledWith({ id: "b" }));

    // Not one that wasn't opened.
    await user.click(await worktreeMenuItem(user, "/w/repo-check", "Remove worktree…"));
    await user.click(await screen.findByRole("button", { name: "Remove" }));
    await vi.waitFor(() => expect(rpc.git.worktree.remove).toHaveBeenCalledTimes(2));
    expect(rpc.repository.remove).toHaveBeenCalledTimes(1);
  });

  it("doesn't remove one when the dialog is cancelled", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await user.click(await worktreeMenuItem(user, "/w/repo-check", "Remove worktree…"));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(rpc.git.worktree.remove).not.toHaveBeenCalled();
  });

  it("doesn't offer removing the main worktree, which is the one on show", async () => {
    const user = userEvent.setup();
    renderSidebar();

    expect(await worktreeMenuItem(user, "/w/repo", "Remove worktree…")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("says beside the section why a worktree couldn't be removed", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "fatal: cannot remove a locked working tree, lock reason: busy";
    rpc.git.worktree.remove.mockRejectedValue(new Error(message));

    await user.click(await worktreeMenuItem(user, "/w/repo-check", "Remove worktree…"));
    await user.click(await screen.findByRole("button", { name: "Remove" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Remove worktree")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    await vi.waitFor(() => expect(screen.queryByText(message)).not.toBeInTheDocument());
  });
});

/** Right-clicks the branch `name`'s row, and chooses Add worktree in its menu. */
async function addWorktreeFor(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.pointer({ keys: "[MouseRight]", target: await branchRow(name) });
  await user.click(await screen.findByRole("menuitem", { name: "Add worktree…" }));
}

describe("worktrees from the branch menu", () => {
  beforeEach(() => {
    rpc.git.worktree.list.mockResolvedValue(WORKTREES);
  });

  it("opens the worktree a branch is checked out in", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.repository.add.mockResolvedValue({
      id: "b",
      name: "repo-feature",
      path: "/w/repo-feature",
    });

    await user.pointer({ keys: "[MouseRight]", target: await branchRow("feature") });
    expect(screen.queryByRole("menuitem", { name: "Add worktree…" })).not.toBeInTheDocument();
    await user.click(await screen.findByRole("menuitem", { name: "Open worktree repo-feature" }));
    expect(rpc.repository.add).toHaveBeenCalledWith({ path: "/w/repo-feature" });
    await vi.waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: "/$repoId", params: { repoId: "b" } }),
    );
  });

  it("opens the worktree a branch is checked out in when it's double-clicked, rather than switching", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: (repository: unknown) => void;
    rpc.repository.add.mockReturnValue(new Promise((resolve) => (finish = resolve)));

    await user.dblClick(await branchRow("feature"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();
    expect(rpc.repository.add).toHaveBeenCalledWith({ path: "/w/repo-feature" });
    await vi.waitFor(() => expect(busySection()).toHaveTextContent("Worktrees"));

    finish({ id: "b", name: "repo-feature", path: "/w/repo-feature" });
    await vi.waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: "/$repoId", params: { repoId: "b" } }),
    );
  });

  it("says beside the worktrees why one couldn't be opened from a branch's menu", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "/w/repo-feature is not a git repository.";
    rpc.repository.add.mockRejectedValue(new Error(message));

    await user.pointer({ keys: "[MouseRight]", target: await branchRow("feature") });
    await user.click(await screen.findByRole("menuitem", { name: "Open worktree repo-feature" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Open worktree")).toBeInTheDocument();
  });

  it("needs a new branch for a remote branch whose tracking branch is checked out", async () => {
    const user = userEvent.setup();
    rpc.git.refs.list.mockResolvedValue([
      branch("main", true),
      { ...branch("feature"), upstream: "origin/feature" },
      branch("origin/feature", false, "remote"),
    ]);
    renderSidebar();

    await addWorktreeFor(user, "origin/feature");
    expect(
      await screen.findByText(
        "feature, which tracks origin/feature, is checked out in repo-feature, so a new branch is made from it and checked out in a folder of its own.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add worktree" })).toBeDisabled();
  });

  it("adds a worktree for a branch, suggesting a folder beside the repository", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.worktree.add.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await addWorktreeFor(user, "origin/feature");
    expect(
      await screen.findByText(
        "Checks out the local branch tracking origin/feature, or a new one that does, in a folder of its own. Name a new branch to make one from it there instead.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^Folder/ })).toHaveValue("/w/repo-origin-feature");
    expect(screen.getByRole("textbox", { name: /^New branch \(optional\)/ })).toHaveValue("");
    await user.click(screen.getByRole("button", { name: "Add worktree" }));
    expect(rpc.git.worktree.add).toHaveBeenCalledWith({
      repositoryId: "a",
      path: "/w/repo-origin-feature",
      branch: "refs/remotes/origin/feature",
    });
    // The dialog closes, and the section shows it running.
    await vi.waitFor(() => expect(screen.queryByText("New worktree")).not.toBeInTheDocument());
    await vi.waitFor(() => expect(busySection()).toHaveTextContent("Worktrees"));

    finish();
    await vi.waitFor(() => expect(busySection()).not.toBeInTheDocument());
  });

  it("needs a new branch for one that's checked out here, and suggests the folder afresh", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.git.worktree.add.mockResolvedValue(undefined);

    await addWorktreeFor(user, "main");
    expect(
      await screen.findByText(
        "main is checked out here, so a new branch is made from it and checked out in a folder of its own.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: /^Folder/ })).toHaveValue("/w/repo-main");
    expect(screen.getByRole("button", { name: "Add worktree" })).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: /^New branch$/ }), "topic");
    await user.click(screen.getByRole("button", { name: "Add worktree with new branch" }));
    expect(rpc.git.worktree.add).toHaveBeenCalledWith({
      repositoryId: "a",
      path: "/w/repo-main",
      branch: "refs/heads/main",
      newBranch: "topic",
    });

    await addWorktreeFor(user, "origin/feature");
    expect(await screen.findByRole("textbox", { name: /^Folder/ })).toHaveValue(
      "/w/repo-origin-feature",
    );
    expect(screen.getByRole("textbox", { name: /^New branch/ })).toHaveValue("");
  });

  it("suggests the folder afresh for another branch, unless one was typed", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await addWorktreeFor(user, "origin/feature");
    expect(await screen.findByRole("textbox", { name: /^Folder/ })).toHaveValue(
      "/w/repo-origin-feature",
    );
    await user.keyboard("{Escape}");
    await addWorktreeFor(user, "main");
    expect(await screen.findByRole("textbox", { name: /^Folder/ })).toHaveValue("/w/repo-main");
    await user.clear(screen.getByRole("textbox", { name: /^Folder/ }));
    await user.type(screen.getByRole("textbox", { name: /^Folder/ }), "/elsewhere/main");
    await user.keyboard("{Escape}");
    await addWorktreeFor(user, "origin/feature");
    expect(await screen.findByRole("textbox", { name: /^Folder/ })).toHaveValue("/elsewhere/main");
  });

  it("picks the folder with the folder picker", async () => {
    const user = userEvent.setup();
    renderSidebar();
    rpc.system.selectFolder.mockResolvedValue("/elsewhere/feature");

    await addWorktreeFor(user, "origin/feature");
    await user.click(await screen.findByRole("button", { name: "Browse…" }));
    await vi.waitFor(() =>
      expect(screen.getByRole("textbox", { name: /^Folder/ })).toHaveValue("/elsewhere/feature"),
    );
  });

  it("says beside the section why a worktree couldn't be added, and keeps the folder to fix", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "fatal: '/w/repo-origin-feature' already exists";
    rpc.git.worktree.add.mockRejectedValue(new Error(message));

    await addWorktreeFor(user, "origin/feature");
    await user.click(await screen.findByRole("button", { name: "Add worktree" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Add worktree")).toBeInTheDocument();

    await addWorktreeFor(user, "origin/feature");
    expect(await screen.findByRole("textbox", { name: /^Folder/ })).toHaveValue(
      "/w/repo-origin-feature",
    );
    // Out of the way of the folder.
    expect(screen.queryByText(message)).not.toBeInTheDocument();
  });
});

/** Right-clicks the branch `name`'s row, and chooses to delete it, which asks first. */
async function deleteFrom(user: ReturnType<typeof userEvent.setup>, name: string) {
  const item = await menuItem(user, name, `Delete ${name}…`);
  // Enabled once the status and the refs have loaded.
  await vi.waitFor(() => expect(item).toHaveAttribute("aria-disabled", "false"));
  await user.click(item);
}

describe("deleting from the branch menu", () => {
  it("deletes a local branch, asking first, showing it running", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.branch.delete.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await deleteFrom(user, "feature");
    expect(await screen.findByText("Delete branch?")).toBeInTheDocument();
    expect(
      screen.getByText("feature is deleted, with any commits only it has."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(rpc.git.branch.delete).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/heads/feature",
    });
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());

    // Nor another one, or a merge, meanwhile.
    expect(await menuItem(user, "origin/feature", "Delete origin/feature…")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(
      screen.getByRole("menuitem", { name: "Merge origin/feature into main" }),
    ).toHaveAttribute("aria-disabled", "true");
    await user.keyboard("{Escape}");

    const refsCalls = rpc.git.refs.list.mock.calls.length;
    finish();
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
    // The repository was reloaded.
    expect(rpc.git.refs.list.mock.calls.length).toBeGreaterThan(refsCalls);
  });

  it("says a local branch's remote one is kept", async () => {
    const user = userEvent.setup();
    rpc.git.refs.list.mockResolvedValue([
      branch("main", true),
      { ...branch("feature"), upstream: "origin/feature" },
      branch("origin/feature", false, "remote"),
    ]);
    renderSidebar();

    await deleteFrom(user, "feature");
    expect(
      await screen.findByText(
        "feature is deleted, with any commits only it has. origin/feature is kept on the remote.",
      ),
    ).toBeInTheDocument();
  });

  it("deletes a remote branch, saying the local branches tracking it go with it", async () => {
    const user = userEvent.setup();
    rpc.git.refs.list.mockResolvedValue([
      branch("main", true),
      { ...branch("feature"), upstream: "origin/feature" },
      { ...branch("feature-copy"), upstream: "origin/feature" },
      branch("origin/feature", false, "remote"),
    ]);
    renderSidebar();
    rpc.git.branch.delete.mockResolvedValue(undefined);

    await deleteFrom(user, "origin/feature");
    expect(await screen.findByText("Delete remote branch?")).toBeInTheDocument();
    expect(
      screen.getByText(
        "origin/feature is deleted on the remote, for everyone, with any commits only it has. So are the local branches feature and feature-copy, which track it.",
      ),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(rpc.git.branch.delete).toHaveBeenCalledWith({
      repositoryId: "a",
      ref: "refs/remotes/origin/feature",
    });
  });

  it("says which commits of the local branches tracking it were never pushed to it", async () => {
    const user = userEvent.setup();
    rpc.git.refs.list.mockResolvedValue([
      branch("main", true),
      { ...branch("feature"), upstream: "origin/feature", ahead: 3 },
      { ...branch("feature-copy"), upstream: "origin/feature" },
      { ...branch("feature-fix"), upstream: "origin/feature", ahead: 1 },
      branch("origin/feature", false, "remote"),
    ]);
    renderSidebar();

    await deleteFrom(user, "origin/feature");
    expect(
      await screen.findByText(
        "origin/feature is deleted on the remote, for everyone, with any commits only it has. So are the local branches feature, feature-copy and feature-fix, which track it. feature has 3 commits and feature-fix has 1 commit that origin/feature doesn't, which are lost unless another branch has them.",
      ),
    ).toBeInTheDocument();
  });

  it("says which commits of a local branch its remote one doesn't have", async () => {
    const user = userEvent.setup();
    rpc.git.refs.list.mockResolvedValue([
      branch("main", true),
      { ...branch("feature"), upstream: "origin/feature", ahead: 2 },
      branch("origin/feature", false, "remote"),
    ]);
    renderSidebar();

    await deleteFrom(user, "feature");
    expect(
      await screen.findByText(
        "feature is deleted, with any commits only it has. origin/feature is kept on the remote. feature has 2 commits that origin/feature doesn't.",
      ),
    ).toBeInTheDocument();
  });

  it("says a branch of a remote that's gone is only deleted here", async () => {
    const user = userEvent.setup();
    rpc.git.refs.list.mockResolvedValue([
      branch("main", true),
      branch("old/feature", false, "remote"),
    ]);
    renderSidebar();

    await deleteFrom(user, "old/feature");
    expect(
      await screen.findByText("old/feature is deleted here: its remote is gone."),
    ).toBeInTheDocument();
  });

  it("doesn't switch branches, nor merge, while it runs", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let finish!: () => void;
    rpc.git.branch.delete.mockReturnValue(new Promise<void>((resolve) => (finish = resolve)));

    await deleteFrom(user, "origin/feature");
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());
    await user.dblClick(await row("feature"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();

    finish();
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
    await user.dblClick(await row("feature"));
    expect(rpc.git.branch.switch).toHaveBeenCalled();
  });

  it("isn't offered until it's known what goes with it, and whether it's checked out", async () => {
    const user = userEvent.setup();
    let loadWorktrees!: (worktrees: Worktree[]) => void;
    rpc.git.worktree.list.mockReturnValue(new Promise((resolve) => (loadWorktrees = resolve)));
    renderSidebar();

    const item = await menuItem(user, "origin/feature", "Delete origin/feature…");
    // The status and the branches have loaded by now, as the merge it offers shows.
    await vi.waitFor(() =>
      expect(
        screen.getByRole("menuitem", { name: "Merge origin/feature into main" }),
      ).toHaveAttribute("aria-disabled", "false"),
    );
    expect(item).toHaveAttribute("aria-disabled", "true");

    loadWorktrees([]);
    await vi.waitFor(() => expect(item).toHaveAttribute("aria-disabled", "false"));
  });

  it("doesn't run while a worktree is being added, which checks out a branch, nor that while it runs", async () => {
    const user = userEvent.setup();
    renderSidebar();
    let added!: () => void;
    rpc.git.worktree.add.mockReturnValue(new Promise<void>((resolve) => (added = resolve)));
    let deleted!: () => void;
    rpc.git.branch.delete.mockReturnValue(new Promise<void>((resolve) => (deleted = resolve)));

    await user.pointer({ keys: "[MouseRight]", target: await row("feature") });
    await user.click(await screen.findByRole("menuitem", { name: "Add worktree…" }));
    await user.click(await screen.findByRole("button", { name: "Add worktree" }));
    await vi.waitFor(() => expect(busySection()).toHaveTextContent("Worktrees"));
    expect(await menuItem(user, "origin/feature", "Delete origin/feature…")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");

    added();
    await vi.waitFor(() => expect(busySection()).not.toBeInTheDocument());
    await deleteFrom(user, "origin/feature");
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    await vi.waitFor(() => expect(busy()).toBeInTheDocument());
    expect(await menuItem(user, "feature", "Add worktree…")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");

    deleted();
    await vi.waitFor(() => expect(busy()).not.toBeInTheDocument());
  });

  it("says a remote branch no local one tracks is deleted on its own", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await deleteFrom(user, "origin/feature");
    expect(
      await screen.findByText(
        "origin/feature is deleted on the remote, for everyone, with any commits only it has.",
      ),
    ).toBeInTheDocument();
  });

  it("doesn't delete it when the dialog is cancelled", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await deleteFrom(user, "feature");
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => expect(screen.queryByText("Delete branch?")).not.toBeInTheDocument());
    expect(rpc.git.branch.delete).not.toHaveBeenCalled();
  });

  it("isn't offered for a branch checked out, here or in another worktree, nor a remote one tracked by it", async () => {
    const user = userEvent.setup();
    rpc.git.worktree.list.mockResolvedValue(WORKTREES);
    rpc.git.refs.list.mockResolvedValue([
      { ...branch("main", true), upstream: "origin/main" },
      { ...branch("feature"), upstream: "origin/feature" },
      branch("topic"),
      branch("origin/main", false, "remote"),
      branch("origin/feature", false, "remote"),
    ]);
    renderSidebar();

    // Enabled for another, once everything has loaded.
    const topic = await menuItem(user, "topic", "Delete topic…");
    await vi.waitFor(() => expect(topic).toHaveAttribute("aria-disabled", "false"));
    await user.keyboard("{Escape}");
    for (const name of ["main", "origin/main", "origin/feature"]) {
      // oxlint-disable-next-line no-await-in-loop -- one menu at a time.
      expect(await menuItem(user, name, `Delete ${name}…`)).toHaveAttribute(
        "aria-disabled",
        "true",
      );
      // oxlint-disable-next-line no-await-in-loop -- one menu at a time.
      await user.keyboard("{Escape}");
    }
    await user.pointer({ keys: "[MouseRight]", target: await branchRow("feature") });
    expect(await screen.findByRole("menuitem", { name: "Delete feature…" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("says why a branch couldn't be deleted, until that's dismissed", async () => {
    const user = userEvent.setup();
    renderSidebar();
    const message = "! [remote rejected] feature (protected branch hook declined)";
    rpc.git.branch.delete.mockRejectedValue(new Error(message));

    await deleteFrom(user, "origin/feature");
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(screen.getByText("Delete branch")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    await vi.waitFor(() => expect(screen.queryByText(message)).not.toBeInTheDocument());
  });

  it("forgets the branch it was asking about deleting in another repository", async () => {
    const user = userEvent.setup();
    const [repositoryId, setRepositoryId] = createSignal("a");
    renderSidebar(repositoryId);

    await deleteFrom(user, "feature");
    expect(await screen.findByText("Delete branch?")).toBeInTheDocument();
    setRepositoryId("b");
    await vi.waitFor(() => expect(screen.queryByText("Delete branch?")).not.toBeInTheDocument());
  });
});
