import type { ChangedFile, Operation, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { Suspense } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";

import { WorkingTreeDetails } from "./working-tree-details";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn(async () => uncommitted) },
    commit: { message: vi.fn(), pushedTo: vi.fn() },
    staging: { discard: vi.fn(async () => {}), discardAll: vi.fn(async () => {}) },
    operation: { get: vi.fn(async (): Promise<Operation | null> => null) },
  },
}));
vi.mock("@/lib/rpc", () => ({ rpc }));

function file(path: string, status: ChangedFile["status"] = "modified"): ChangedFile {
  return { path, status, origPath: null, additions: 1, deletions: 0 };
}

let uncommitted: Uncommitted;
function setChanges(unstaged: ChangedFile[], staged: ChangedFile[] = []) {
  const conflicted = unstaged.filter((changed) => changed.status === "conflicted").length;
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: {
      files: unstaged.length + staged.length,
      staged: staged.length,
      unstaged: unstaged.length,
      conflicted,
    },
    changes: { staged, unstaged, uncounted: false, markerFree: [] },
    version: String(Math.random()),
  };
}

/** The details of the uncommitted changes, as `uncommitted` has them. */
function renderDetails() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(() => (
    <QueryClientProvider client={client}>
      <Suspense>
        <WorkingTreeDetails repositoryId="repo" />
      </Suspense>
    </QueryClientProvider>
  ));
}

describe("the uncommitted changes' details", () => {
  it("stay on the page while the status is refetched, so their lists keep their scroll position", async () => {
    setChanges([file("a.txt"), file("b.txt")]);
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    client.setQueryDefaults(gitKeys.all, { staleTime: Infinity });
    // Loaded already, by the history's row for them, as it is when they're selected.
    await client.prefetchQuery({
      queryKey: gitKeys.status("repo"),
      queryFn: async () => uncommitted,
    });
    render(() => (
      <QueryClientProvider client={client}>
        <Suspense>
          <WorkingTreeDetails repositoryId="repo" />
        </Suspense>
      </QueryClientProvider>
    ));
    await screen.findByText(/2 unstaged/);
    const list = document.querySelector("[data-scroll-restoration-id=unstaged-files]")!;
    // Suspending takes the details off the page and puts them back, their lists scrolled to the top.
    const removed: Node[] = [];
    const observer = new MutationObserver((records) => {
      for (const record of records) removed.push(...record.removedNodes);
    });
    observer.observe(document.body, { childList: true, subtree: true });

    // A file saved in an editor: the first refetch after the details came on show.
    setChanges([file("a.txt"), file("b.txt"), file("c.txt")]);
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    expect(await screen.findByText(/3 unstaged/)).toBeInTheDocument();
    observer.disconnect();
    expect(removed.filter((node) => node.contains(list))).toEqual([]);
  });
});

/** Opens the menu of the file at `path` by right-clicking it in its list. */
async function openMenu(user: ReturnType<typeof userEvent.setup>, path: string) {
  const row = await screen.findByText(path);
  await user.pointer({ keys: "[MouseRight]", target: row });
}

describe("discarding changes", () => {
  beforeEach(() => {
    // jsdom has no layout: give the lists room for their rows.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
    rpc.git.staging.discard.mockClear();
    rpc.git.staging.discardAll.mockClear();
    rpc.git.operation.get.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("discards a file's changes from its menu, by the side it's listed on, once that's confirmed", async () => {
    const user = userEvent.setup();
    setChanges([file("a.txt"), file("new.txt", "untracked")], [file("b.txt")]);
    renderDetails();

    await openMenu(user, "new.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "new.txt isn't tracked, so it's deleted for good.",
    );
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(rpc.git.staging.discard).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "new.txt",
      origPath: null,
      side: "unstaged",
    });

    await openMenu(user, "b.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    await user.click(await screen.findByRole("button", { name: "Discard" }));
    expect(rpc.git.staging.discard).toHaveBeenLastCalledWith({
      repositoryId: "repo",
      path: "b.txt",
      origPath: null,
      side: "staged",
    });
  });

  it("discards a copy as it's listed, leaving its source to the git side", async () => {
    const user = userEvent.setup();
    setChanges([], [{ ...file("copy.txt", "copied"), origPath: "a.txt" }]);
    renderDetails();

    await openMenu(user, "copy.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    await user.click(await screen.findByRole("button", { name: "Discard" }));
    expect(rpc.git.staging.discard).toHaveBeenCalledWith({
      repositoryId: "repo",
      path: "copy.txt",
      origPath: "a.txt",
      side: "staged",
    });
  });

  it("discards nothing when it's cancelled, and nothing of a conflicted file", async () => {
    const user = userEvent.setup();
    setChanges([file("a.txt"), file("c.txt", "conflicted")]);
    renderDetails();

    await openMenu(user, "a.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());

    await openMenu(user, "c.txt");
    expect(await screen.findByRole("menuitem", { name: "Discard changes…" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    expect(rpc.git.staging.discard).not.toHaveBeenCalled();
  });

  it("discards all changes from the header once that's confirmed", async () => {
    const user = userEvent.setup();
    setChanges([file("a.txt")], [file("b.txt")]);
    renderDetails();

    await user.click(await screen.findByRole("button", { name: "Discard all changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("Discard all changes?");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(rpc.git.staging.discardAll).toHaveBeenCalledWith({ repositoryId: "repo" });
  });

  it("doesn't discard all changes while files are conflicted, and says so", async () => {
    setChanges([file("a.txt"), file("c.txt", "conflicted")]);
    renderDetails();

    const button = await screen.findByRole("button", { name: "Discard all changes…" });
    await vi.waitFor(() =>
      expect(button).toHaveAttribute(
        "title",
        "Resolve the conflicts before discarding all changes.",
      ),
    );
    expect(button).toBeDisabled();
  });

  it("doesn't discard a file's staged changes while a merge is under way, but its unstaged ones", async () => {
    const user = userEvent.setup();
    rpc.git.operation.get.mockResolvedValue({ kind: "merge", merging: "side", into: "main" });
    setChanges([file("a.txt")], [file("b.txt")]);
    renderDetails();
    // Once the operation's loaded.
    await vi.waitFor(() => expect(rpc.git.operation.get).toHaveBeenCalled());

    await openMenu(user, "b.txt");
    await vi.waitFor(() =>
      expect(screen.getByRole("menuitem", { name: "Discard changes…" })).toHaveAttribute(
        "aria-disabled",
        "true",
      ),
    );
    await user.keyboard("{Escape}");
    await vi.waitFor(() => expect(screen.queryByRole("menuitem")).not.toBeInTheDocument());
    await openMenu(user, "a.txt");
    expect(await screen.findByRole("menuitem", { name: "Discard changes…" })).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("doesn't discard all changes while a merge is under way, which would go on without them", async () => {
    rpc.git.operation.get.mockResolvedValue({ kind: "merge", merging: "side", into: "main" });
    setChanges([file("a.txt")]);
    renderDetails();

    const button = await screen.findByRole("button", { name: "Discard all changes…" });
    await vi.waitFor(() =>
      expect(button).toHaveAttribute(
        "title",
        "Finish or abort the merge before discarding all changes.",
      ),
    );
    expect(button).toBeDisabled();
  });
});
