import type { ChangedFile, Operation, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { Suspense } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";

import { WorkingTreeDetails } from "./working-tree-details";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn(async () => uncommitted) },
    commit: { message: vi.fn(), pushedTo: vi.fn() },
    staging: {
      stage: vi.fn(async () => {}),
      discard: vi.fn(async (): Promise<void> => {}),
      discardAll: vi.fn(async () => {}),
    },
    operation: { get: vi.fn(async (): Promise<Operation | null> => null) },
  },
}));
vi.mock("@/lib/rpc", () => ({ rpc }));

function file(
  path: string,
  status: ChangedFile["status"] = "modified",
  origPath: string | null = null,
): ChangedFile {
  return { path, status, origPath, additions: 1, deletions: 0 };
}

let uncommitted: Uncommitted;
function setChanges(unstaged: ChangedFile[], staged: ChangedFile[] = []) {
  const conflicted = unstaged.filter((change) => change.status === "conflicted").length;
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: {
      files: unstaged.length + staged.length,
      staged: staged.length,
      unstaged: unstaged.length - conflicted,
      conflicted,
    },
    changes: { staged, unstaged, uncounted: false, markerFree: [] },
    version: String(Math.random()),
  };
}

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

/** Right-clicks the file `name` in the list marked `scrollId`. */
async function openMenu(user: ReturnType<typeof userEvent.setup>, scrollId: string, name: string) {
  const list = await vi.waitFor(() => {
    const element = document.querySelector(`[data-scroll-restoration-id=${scrollId}]`);
    if (!element?.textContent?.includes(name)) throw new Error(`${name} isn't listed yet`);
    return element;
  });
  const row = [...list.querySelectorAll("[data-file]")].find((element) =>
    element.textContent?.includes(name),
  )!;
  await user.pointer({ keys: "[MouseRight]", target: row });
}

beforeEach(() => {
  // jsdom has no layout: give the lists room for their rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(300);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(200);
  rpc.git.staging.discard.mockReset();
  rpc.git.staging.discardAll.mockClear();
  rpc.git.operation.get.mockResolvedValue(null);
});

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

describe("discarding changes", () => {
  it("discards a file's unstaged changes from its menu, once asked", async () => {
    const user = userEvent.setup();
    setChanges([file("src/a.txt"), file("b.txt")]);
    renderDetails();

    await openMenu(user, "unstaged-files", "a.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "The unstaged changes to src/a.txt are lost.",
    );
    expect(rpc.git.staging.discard).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Discard" }));
    await vi.waitFor(() =>
      expect(rpc.git.staging.discard).toHaveBeenCalledWith({
        repositoryId: "repo",
        path: "src/a.txt",
        origPath: null,
        status: "modified",
        side: "unstaged",
      }),
    );
  });

  it("says an untracked file is deleted, and puts a staged one back as HEAD has it", async () => {
    const user = userEvent.setup();
    setChanges([file("new.txt", "untracked")], [file("staged.txt")]);
    renderDetails();

    await openMenu(user, "unstaged-files", "new.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "new.txt isn't tracked, so it's deleted.",
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());

    await openMenu(user, "staged-files", "staged.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    await user.click(await screen.findByRole("button", { name: "Discard" }));
    await vi.waitFor(() =>
      expect(rpc.git.staging.discard).toHaveBeenCalledWith(
        expect.objectContaining({ path: "staged.txt", side: "staged" }),
      ),
    );
    expect(rpc.git.staging.discard).toHaveBeenCalledTimes(1);
  });

  it("doesn't discard a conflicted file's changes, nor all of them while there are conflicts", async () => {
    const user = userEvent.setup();
    setChanges([file("both.txt", "conflicted"), file("a.txt")]);
    renderDetails();

    await openMenu(user, "unstaged-files", "both.txt");
    expect(await screen.findByRole("menuitem", { name: "Discard changes…" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.keyboard("{Escape}");
    expect(
      screen.getByRole("button", { name: "Resolve the conflicts before discarding all changes" }),
    ).toBeDisabled();
  });

  it("discards all changes, once asked", async () => {
    const user = userEvent.setup();
    setChanges([file("a.txt")], [file("b.txt")]);
    renderDetails();

    const discardAll = await screen.findByRole("button", { name: "Discard all changes…" });
    await vi.waitFor(() => expect(discardAll).toBeEnabled());
    await user.click(discardAll);
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("untracked files are deleted");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    await vi.waitFor(() =>
      expect(rpc.git.staging.discardAll).toHaveBeenCalledWith({ repositoryId: "repo" }),
    );
  });

  it("can't discard all changes when there are none, but repositories inside this one", async () => {
    setChanges([file("vendor/lib/", "untracked")]);
    renderDetails();
    await screen.findByText(/1 unstaged/);
    expect(screen.getByRole("button", { name: "No changes to discard" })).toBeDisabled();
  });

  it("can't discard all changes while an operation is under way", async () => {
    rpc.git.operation.get.mockResolvedValue({ kind: "merge", merging: "side", into: "main" });
    setChanges([file("a.txt")]);
    renderDetails();
    expect(
      await screen.findByRole("button", {
        name: "Finish or abort the merge before discarding all changes",
      }),
    ).toBeDisabled();
  });

  it("says a renamed file is put back at its old path, and a copy deleted", async () => {
    const user = userEvent.setup();
    setChanges([], [file("new.txt", "renamed", "old.txt"), file("copy.txt", "copied", "lib.txt")]);
    renderDetails();

    await openMenu(user, "staged-files", "new.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "new.txt is deleted, and old.txt put back as the last commit has it",
    );
    await user.click(screen.getByRole("button", { name: "Discard" }));
    await vi.waitFor(() =>
      expect(rpc.git.staging.discard).toHaveBeenLastCalledWith(
        expect.objectContaining({ path: "new.txt", origPath: "old.txt" }),
      ),
    );

    await openMenu(user, "staged-files", "copy.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "copy.txt is new, so it's deleted",
    );
    await user.click(screen.getByRole("button", { name: "Discard" }));
    await vi.waitFor(() =>
      expect(rpc.git.staging.discard).toHaveBeenLastCalledWith(
        expect.objectContaining({ path: "copy.txt", status: "copied" }),
      ),
    );
  });

  it("shows why the latest action failed, and no longer once another goes through", async () => {
    const user = userEvent.setup();
    rpc.git.staging.discard.mockRejectedValue(new Error("Couldn't discard it."));
    setChanges([file("a.txt"), file("b.txt")]);
    renderDetails();

    await openMenu(user, "unstaged-files", "a.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    await user.click(await screen.findByRole("button", { name: "Discard" }));
    expect(await screen.findByText("Couldn't discard it.")).toBeInTheDocument();

    // Once the dialog is gone, which hides the rest from the pointer while it's open.
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    await user.click(await screen.findByRole("button", { name: "Stage b.txt" }));
    await vi.waitFor(() => expect(rpc.git.staging.stage).toHaveBeenCalled());
    await vi.waitFor(() =>
      expect(screen.queryByText("Couldn't discard it.")).not.toBeInTheDocument(),
    );
  });

  it("asks about a file as its list has it now, and no longer once it's gone", async () => {
    const user = userEvent.setup();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    setChanges([file("a.txt"), file("b.txt")]);
    render(() => (
      <QueryClientProvider client={client}>
        <Suspense>
          <WorkingTreeDetails repositoryId="repo" />
        </Suspense>
      </QueryClientProvider>
    ));

    await openMenu(user, "unstaged-files", "a.txt");
    await user.click(await screen.findByRole("menuitem", { name: "Discard changes…" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent("unstaged changes to a.txt");

    // Untracked in a terminal: discarding it deletes it now.
    setChanges([file("a.txt", "untracked"), file("b.txt")]);
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    await vi.waitFor(() =>
      expect(screen.getByRole("alertdialog")).toHaveTextContent("a.txt isn't tracked"),
    );

    setChanges([file("b.txt")]);
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    await vi.waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(rpc.git.staging.discard).not.toHaveBeenCalled();
  });
});
