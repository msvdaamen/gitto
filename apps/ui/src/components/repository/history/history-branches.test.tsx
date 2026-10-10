import type { CommitRef, Ref, Worktree } from "@gitto/git/types";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import type * as SolidRouter from "@tanstack/solid-router";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BranchMenuProvider } from "../branch-menu";
import { RefList } from "../sidebar/ref-list";
import { HistoryTable } from "./history-table";

/** The one commit's branches and tags: `main` is checked out, the rest only show under `+3`. */
const REFS: CommitRef[] = [
  { kind: "local", name: "main", fullName: "refs/heads/main", current: true },
  { kind: "local", name: "feature", fullName: "refs/heads/feature" },
  { kind: "remote", name: "origin/release", fullName: "refs/remotes/origin/release" },
  { kind: "tag", name: "v1", fullName: "refs/tags/v1" },
];

function ref(fullName: string, kind: Ref["kind"]): Ref {
  return {
    name: fullName.split("/").slice(2).join("/"),
    fullName,
    kind,
    sha: "e5",
    current: fullName === "refs/heads/main",
    upstream: null,
    ahead: 0,
    behind: 0,
  };
}

const rpc = vi.hoisted(() => ({
  repository: {
    list: async () => [{ id: "repo", name: "repo", path: "/w/repo" }],
    add: vi.fn(),
  },
  git: {
    history: { log: vi.fn() },
    status: { get: vi.fn() },
    diff: { commitFiles: async () => [] },
    stash: { list: async () => [] },
    refs: { list: vi.fn() },
    branch: { switch: vi.fn(), create: vi.fn(), merge: vi.fn() },
    operation: { get: vi.fn() },
    worktree: { list: vi.fn(), add: vi.fn(), remove: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** Where the router was sent, e.g. to the worktree opened. */
const navigate = vi.hoisted(() => vi.fn());

vi.mock("@tanstack/solid-router", async (importOriginal) => ({
  ...(await importOriginal<typeof SolidRouter>()),
  useNavigate: () => navigate,
}));

beforeEach(() => {
  // jsdom has no layout: give the history room for its rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(700);
  rpc.git.history.log.mockResolvedValue([
    {
      sha: "e5",
      parents: [],
      authorName: "Ada Lovelace",
      authorEmail: "ada@example.com",
      authoredAt: 0,
      committedAt: 0,
      refs: REFS,
      subject: "Commit e5",
      body: "",
    },
  ]);
  rpc.git.status.get.mockResolvedValue({
    head: { kind: "branch", name: "main", sha: "e5" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
    version: "1",
  });
  rpc.git.refs.list.mockResolvedValue([
    ref("refs/heads/main", "local"),
    ref("refs/heads/feature", "local"),
    ref("refs/remotes/origin/release", "remote"),
    ref("refs/tags/v1", "tag"),
  ]);
  rpc.git.branch.switch.mockResolvedValue(undefined);
  rpc.git.branch.create.mockResolvedValue(undefined);
  rpc.git.branch.merge.mockResolvedValue("merged");
  rpc.git.operation.get.mockResolvedValue(null);
  rpc.git.worktree.list.mockResolvedValue([]);
});

afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
});

/**
 * The history, with the list focused, and the sidebar with `sidebar`; in the repository `repo`
 * until `setRepositoryId` switches it.
 */
async function renderHistory(options: { sidebar?: boolean } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const [repositoryId, setRepositoryId] = createSignal("repo");
  const [selectedId, setSelectedId] = createSignal<string>();
  render(() => (
    <QueryClientProvider client={client}>
      <BranchMenuProvider repositoryId={repositoryId()}>
        {options.sidebar && <RefList repositoryId={repositoryId()} />}
        <HistoryTable
          repositoryId={repositoryId()}
          search=""
          selectedId={selectedId()}
          detailsId={undefined}
          onSelect={setSelectedId}
        />
      </BranchMenuProvider>
    </QueryClientProvider>
  ));
  const list = await screen.findByRole("listbox", { name: "Commit history" });
  await vi.waitFor(() => expect(selectedId()).toBe("e5"));
  list.focus();
  return { list, setRepositoryId };
}

/** Waits a moment for a menu to open, if one did. */
async function aMoment() {
  // oxlint-disable-next-line no-promise-executor-return -- a moment for it to open, if it did.
  await new Promise((resolve) => setTimeout(resolve, 50));
}

/** Hovers the count of the labels the row has no room for, and finds `name` among them. */
async function fromTheRest(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.hover(await screen.findByText("+3"));
  return screen.findByText(name);
}

/** Right-clicks `target`, and chooses Create branch in its menu. */
async function createFrom(user: ReturnType<typeof userEvent.setup>, target: Element) {
  await user.pointer({ keys: "[MouseRight]", target });
  await user.click(await screen.findByRole("menuitem", { name: "Create branch…" }));
}

describe("branches in the history", () => {
  it("switches to a branch double-clicked among the rest of a row's labels", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await user.dblClick(await fromTheRest(user, "origin/release"));
    expect(rpc.git.branch.switch).toHaveBeenCalledWith({
      repositoryId: "repo",
      ref: "refs/remotes/origin/release",
    });
  });

  it("creates a branch from the one right-clicked, handing the focus back to the list", async () => {
    const user = userEvent.setup();
    const { list } = await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("main") });
    await screen.findByRole("menuitem", { name: "Create branch…" });
    await user.keyboard("{Escape}");
    await vi.waitFor(() => expect(list).toHaveFocus());

    await createFrom(user, screen.getByText("main"));
    expect(await screen.findByText("From main.")).toBeInTheDocument();
    await user.type(await screen.findByRole("textbox", { name: "Name" }), "topic{Enter}");
    expect(rpc.git.branch.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      name: "topic",
      from: "refs/heads/main",
    });
  });

  it("creates a branch from one right-clicked among the rest of a row's labels", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await createFrom(user, await fromTheRest(user, "feature"));
    await user.type(await screen.findByRole("textbox", { name: "Name" }), "topic{Enter}");
    expect(rpc.git.branch.create).toHaveBeenCalledWith({
      repositoryId: "repo",
      name: "topic",
      from: "refs/heads/feature",
    });
  });

  it("merges a branch right-clicked among the rest of a row's labels into the checked-out one", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await user.pointer({
      keys: "[MouseRight]",
      target: await fromTheRest(user, "origin/release"),
    });
    const item = await screen.findByRole("menuitem", { name: "Merge origin/release into main" });
    await vi.waitFor(() => expect(item).toHaveAttribute("aria-disabled", "false"));
    await user.click(item);
    expect(rpc.git.branch.merge).toHaveBeenCalledWith({
      repositoryId: "repo",
      ref: "refs/remotes/origin/release",
      into: "main",
    });
  });

  it("doesn't offer merging the checked-out branch's label into itself", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("main") });
    await screen.findByRole("menuitem", { name: "Create branch…" });
    expect(screen.queryByRole("menuitem", { name: /^Merge/ })).not.toBeInTheDocument();
  });

  it("opens no menu for a tag", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await fromTheRest(user, "v1") });
    await aMoment();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("opens the selected row's branch's menu with a key", async () => {
    const user = userEvent.setup();
    const { list } = await renderHistory();

    // What Shift+F10 or the Menu key fires, at what has the focus.
    fireEvent.contextMenu(list);
    await user.click(await screen.findByRole("menuitem", { name: "Create branch…" }));
    expect(await screen.findByText("From main.")).toBeInTheDocument();
  });

  it("opens no menu at a long press off a branch", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      await renderHistory();
      fireEvent.pointerDown(screen.getByText("Commit e5"), { pointerType: "touch" });
      await vi.advanceTimersByTimeAsync(1000);
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes the menu once the repository is switched", async () => {
    const user = userEvent.setup();
    const { setRepositoryId } = await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("main") });
    await screen.findByRole("menuitem", { name: "Create branch…" });
    setRepositoryId("other");
    await vi.waitFor(() => expect(screen.queryByRole("menu")).not.toBeInTheDocument());
  });

  it("keeps a name to fix after a failure, whether opened from the history or the sidebar", async () => {
    const user = userEvent.setup();
    const message = "fatal: a branch named 'topic' already exists";
    rpc.git.branch.create.mockRejectedValue(new Error(message));
    const { list } = await renderHistory({ sidebar: true });

    await createFrom(user, await within(list).findByText("main"));
    await user.type(await screen.findByRole("textbox", { name: "Name" }), "topic{Enter}");
    await vi.waitFor(() =>
      expect(screen.queryByRole("textbox", { name: "Name" })).not.toBeInTheDocument(),
    );

    await createFrom(user, await screen.findByTitle("feature"));
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("topic");
  });
});

function worktree(path: string, branch: string | null, extra: Partial<Worktree> = {}): Worktree {
  return {
    path,
    name: path.slice(path.lastIndexOf("/") + 1),
    head: "e5",
    branch,
    main: false,
    bare: false,
    current: false,
    locked: null,
    prunable: null,
    ...extra,
  };
}

describe("worktrees in the history", () => {
  beforeEach(() => {
    rpc.git.worktree.list.mockResolvedValue([
      worktree("/w/repo", "refs/heads/main", { main: true, current: true }),
      worktree("/w/repo-feature", "refs/heads/feature"),
      worktree("/w/repo-check", null),
    ]);
  });

  it("marks a branch checked out in another worktree, and names one detached at the commit", async () => {
    const user = userEvent.setup();
    await renderHistory();

    // The worktree is a label of its own, after the branches.
    await user.hover(await screen.findByText("+4"));
    const feature = (await screen.findByText("feature")).closest("[data-branch]")!;
    expect(
      within(feature as HTMLElement).getByRole("img", { name: "Checked out in repo-feature" }),
    ).toBeInTheDocument();
    expect(feature).toHaveAttribute(
      "title",
      "feature (local; checked out in repo-feature)\nDouble-click to open that worktree",
    );
    const check = (await screen.findByText("repo-check")).parentElement!;
    expect(within(check).getByRole("img", { name: "Worktree" })).toBeInTheDocument();
    expect(check).toHaveAttribute("title", "repo-check (worktree, detached)");
    expect(check).not.toHaveAttribute("data-branch");
  });

  it("opens the worktree a branch is checked out in when its label is double-clicked", async () => {
    const user = userEvent.setup();
    await renderHistory();
    rpc.repository.add.mockResolvedValue({
      id: "b",
      name: "repo-feature",
      path: "/w/repo-feature",
    });

    await user.hover(await screen.findByText("+4"));
    await user.dblClick(await screen.findByText("feature"));
    expect(rpc.git.branch.switch).not.toHaveBeenCalled();
    expect(rpc.repository.add).toHaveBeenCalledWith({ path: "/w/repo-feature" });
    await vi.waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: "/$repoId", params: { repoId: "b" } }),
    );
  });

  it("opens the worktree a branch is checked out in from its menu", async () => {
    const user = userEvent.setup();
    await renderHistory();
    rpc.repository.add.mockResolvedValue({
      id: "b",
      name: "repo-feature",
      path: "/w/repo-feature",
    });

    await user.hover(await screen.findByText("+4"));
    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("feature") });
    await user.click(await screen.findByRole("menuitem", { name: "Open worktree repo-feature" }));
    expect(rpc.repository.add).toHaveBeenCalledWith({ path: "/w/repo-feature" });
    await vi.waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ to: "/$repoId", params: { repoId: "b" } }),
    );
  });

  it("adds a worktree for a branch from its menu", async () => {
    const user = userEvent.setup();
    await renderHistory();
    rpc.git.worktree.add.mockResolvedValue(undefined);

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("main") });
    await user.click(await screen.findByRole("menuitem", { name: "Add worktree…" }));
    await user.type(await screen.findByRole("textbox", { name: /^New branch$/ }), "topic");
    await user.click(screen.getByRole("button", { name: "Add worktree with new branch" }));
    expect(rpc.git.worktree.add).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "/w/repo-main",
      branch: "refs/heads/main",
      newBranch: "topic",
    });
  });
});
