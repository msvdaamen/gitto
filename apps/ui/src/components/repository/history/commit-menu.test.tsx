import type { CommitRef, Head } from "@gitto/git/types";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createEffect, createRoot, createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { WIP_ID } from "@/git/rows";

import { BranchMenuProvider } from "../branch-menu";
import { HistoryTable } from "./history-table";

const MAIN: CommitRef = { kind: "local", name: "main", fullName: "refs/heads/main", current: true };

/** A commit of the log, `sha`, with `parents`. */
function logCommit(sha: string, subject: string, parents: string[], refs: CommitRef[] = []) {
  return {
    sha,
    parents,
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: 0,
    committedAt: 0,
    refs,
    subject,
    body: "",
  };
}

const rpc = vi.hoisted(() => ({
  repository: { list: async () => [{ id: "repo", name: "repo", path: "/w/repo" }] },
  git: {
    history: { log: vi.fn() },
    status: { get: vi.fn() },
    diff: { commitFiles: async () => [] },
    stash: { list: async () => [] },
    refs: { list: async () => [] },
    branch: { switch: vi.fn() },
    operation: { get: vi.fn() },
    worktree: { list: async () => [] },
    commit: { message: vi.fn(), pushedTo: vi.fn(), reword: vi.fn() },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** HEAD, as the status says. */
let head: Head;
/** How many files the status says are staged, all that's changed. */
let staged: number;

beforeEach(() => {
  // jsdom has no layout: give the history room for its rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(700);
  head = { kind: "branch", name: "main", sha: "e5" };
  staged = 0;
  rpc.git.history.log.mockResolvedValue([
    logCommit("e5", "Commit e5", ["d4"], [MAIN]),
    logCommit("d4", "Commit d4", []),
  ]);
  rpc.git.status.get.mockImplementation(async () => ({
    head,
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: staged, staged, unstaged: 0, conflicted: 0 },
    changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
    version: "1",
  }));
  rpc.git.operation.get.mockResolvedValue(null);
  rpc.git.commit.message.mockResolvedValue("Commit e5\n\nWhy it was made\n");
  rpc.git.commit.pushedTo.mockResolvedValue(null);
});

afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
});

/** The history, with HEAD's commit selected; what's selected next is `selectedId`. */
async function renderHistory() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // Rather than the uncommitted changes, which come first if there are any.
  const [selectedId, setSelectedId] = createSignal<string>("e5");
  render(() => (
    <QueryClientProvider client={client}>
      <BranchMenuProvider repositoryId="repo">
        <HistoryTable
          repositoryId="repo"
          search=""
          selectedId={selectedId()}
          detailsId={undefined}
          onSelect={setSelectedId}
        />
      </BranchMenuProvider>
    </QueryClientProvider>
  ));
  await screen.findByRole("listbox", { name: "Commit history" });
  await vi.waitFor(() => expect(selectedId()).toBe("e5"));
  return { selectedId };
}

/** Waits a moment for a menu to open, if one did. */
async function aMoment() {
  // oxlint-disable-next-line no-promise-executor-return -- a moment for it to open, if it did.
  await new Promise((resolve) => setTimeout(resolve, 50));
}

/** Right-clicks the commit `subject`, and chooses Edit message in its menu once it can be. */
async function editMessageOf(user: ReturnType<typeof userEvent.setup>, subject: string) {
  await user.pointer({ keys: "[MouseRight]", target: await screen.findByText(subject) });
  const item = await screen.findByRole("menuitem", { name: "Edit message…" });
  await vi.waitFor(() => expect(item).toHaveAttribute("aria-disabled", "false"));
  await user.click(item);
  // By its title: Kobalte gives every dialog the role of an alert dialog once one is loaded.
  return screen.findByText("Edit commit message");
}

describe("the commit menu", () => {
  it("edits the last commit's message, keeping the rewritten commit selected", async () => {
    const user = userEvent.setup();
    const { selectedId } = await renderHistory();
    rpc.git.commit.reword.mockImplementation(async () => {
      head = { kind: "branch", name: "main", sha: "f6" };
      rpc.git.history.log.mockResolvedValue([
        logCommit("f6", "Reworded", ["d4"], [MAIN]),
        logCommit("d4", "Commit d4", []),
      ]);
      return "f6";
    });

    await editMessageOf(user, "Commit e5");
    const summary = screen.getByRole("textbox", { name: "Commit message" });
    await vi.waitFor(() => expect(summary).toHaveValue("Commit e5"));
    expect(screen.getByRole("textbox", { name: /^Description/ })).toHaveValue("Why it was made");
    const save = screen.getByRole("button", { name: "Save message" });
    // Nothing to save until it's edited.
    expect(save).toBeDisabled();

    await user.clear(summary);
    await user.type(summary, "Reworded");
    await user.click(save);
    expect(rpc.git.commit.reword).toHaveBeenCalledWith({
      repositoryId: "repo",
      sha: "e5",
      message: "Reworded\n\nWhy it was made\n",
    });
    await vi.waitFor(() =>
      expect(screen.queryByText("Edit commit message")).not.toBeInTheDocument(),
    );
    await vi.waitFor(() => expect(selectedId()).toBe("f6"));
  });

  it("keeps the rewritten commit selected rather than the uncommitted changes", async () => {
    const user = userEvent.setup();
    staged = 1;
    const { selectedId } = await renderHistory();
    // The history has a row for the changes, above the commit.
    await screen.findByText("Commit e5");
    rpc.git.commit.reword.mockImplementation(async () => {
      head = { kind: "branch", name: "main", sha: "f6" };
      rpc.git.history.log.mockResolvedValue([
        logCommit("f6", "Reworded", ["d4"], [MAIN]),
        logCommit("d4", "Commit d4", []),
      ]);
      return "f6";
    });
    const selections: (string | undefined)[] = [];
    createRoot(() => createEffect(() => selections.push(selectedId())));

    await editMessageOf(user, "Commit e5");
    const summary = screen.getByRole("textbox", { name: "Commit message" });
    await vi.waitFor(() => expect(summary).toHaveValue("Commit e5"));
    await user.type(summary, "!{Enter}");
    await vi.waitFor(() => expect(selectedId()).toBe("f6"));
    // The history fell back to the changes once the commit was gone, before it was told of f6.
    expect(selections).toEqual(["e5", WIP_ID, "f6"]);
  });

  it("says when the commit is pushed already", async () => {
    const user = userEvent.setup();
    rpc.git.commit.pushedTo.mockResolvedValue("origin/main");
    await renderHistory();

    await editMessageOf(user, "Commit e5");
    expect(
      await screen.findByText(
        "The commit is already on origin/main; editing its message means force-pushing.",
      ),
    ).toBeInTheDocument();
  });

  it("stays open with the edits if rewording fails, saying why", async () => {
    const user = userEvent.setup();
    const message = "The last commit changed before it could be amended, so nothing was amended.";
    rpc.git.commit.reword.mockRejectedValue(new Error(message));
    await renderHistory();

    await editMessageOf(user, "Commit e5");
    const summary = screen.getByRole("textbox", { name: "Commit message" });
    await vi.waitFor(() => expect(summary).toHaveValue("Commit e5"));
    await user.type(summary, "!{Enter}");
    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(summary).toHaveValue("Commit e5!");
  });

  it("is only the last commit of the checked-out branch's", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("Commit d4") });
    await aMoment();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("isn't there with HEAD detached", async () => {
    const user = userEvent.setup();
    head = { kind: "detached", sha: "e5" };
    await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("Commit e5") });
    await aMoment();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("can't edit the message while an operation is under way", async () => {
    const user = userEvent.setup();
    rpc.git.operation.get.mockResolvedValue({ kind: "merge" });
    await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("Commit e5") });
    const item = await screen.findByRole("menuitem", { name: "Edit message…" });
    expect(item).toHaveAttribute("aria-disabled", "true");
  });

  it("leaves a branch's label in the row to the branch's menu", async () => {
    const user = userEvent.setup();
    await renderHistory();

    await user.pointer({ keys: "[MouseRight]", target: await screen.findByText("main") });
    await screen.findByRole("menuitem", { name: "Create branch…" });
    expect(screen.queryByRole("menuitem", { name: "Edit message…" })).not.toBeInTheDocument();
  });

  it("opens at a long press of the row", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      await renderHistory();
      fireEvent.pointerDown(screen.getByText("Commit e5"), { pointerType: "touch" });
      await vi.advanceTimersByTimeAsync(1000);
      expect(screen.getByRole("menuitem", { name: "Edit message…" })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("opens the last commit's menu with a key, rather than its branch's", async () => {
    const user = userEvent.setup();
    await renderHistory();
    const list = screen.getByRole("listbox", { name: "Commit history" });
    list.focus();

    // What Shift+F10 or the Menu key fires, at what has the focus.
    fireEvent.contextMenu(list);
    await screen.findByRole("menuitem", { name: "Edit message…" });
    expect(screen.queryByRole("menuitem", { name: "Create branch…" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Edit message…" }));
    expect(await screen.findByText("Edit commit message")).toBeInTheDocument();
  });
});
