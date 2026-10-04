import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HistoryTable } from "./history-table";

const SHAS = ["e5", "d4", "c3", "b2", "a1"];

const rpc = vi.hoisted(() => ({
  /** The commits whose files were asked for. */
  fileCalls: [] as string[],
  git: {
    history: {
      log: async () =>
        ["e5", "d4", "c3", "b2", "a1"].map((sha, index, shas) => ({
          sha,
          parents: shas[index + 1] ? [shas[index + 1]] : [],
          authorName: "Ada Lovelace",
          authorEmail: "ada@example.com",
          authoredAt: 0,
          committedAt: 5000 - index * 1000,
          refs: [],
          subject: `Commit ${sha}`,
          body: "",
        })),
    },
    status: {
      get: async () => ({
        head: { kind: "branch", name: "main", sha: "e5" },
        upstream: null,
        ahead: 0,
        behind: 0,
        counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
        changes: { staged: [], unstaged: [], uncounted: false },
        version: "1",
      }),
    },
    diff: {
      commitFiles: async ({ sha }: { sha: string }) => {
        rpc.fileCalls.push(sha);
        return [];
      },
    },
    stash: { list: async () => [] },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** The history, with the list focused and its first commit selected. */
async function renderHistory(search = "") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const [selectedId, setSelectedId] = createSignal<string>();
  // The row the details show, which the test moves itself: it lags behind a selection on the move.
  const [detailsId, setDetailsId] = createSignal<string>();
  render(() => (
    <QueryClientProvider client={client}>
      <HistoryTable
        repositoryId="repo"
        search={search}
        selectedId={selectedId()}
        detailsId={detailsId()}
        onSelect={setSelectedId}
      />
    </QueryClientProvider>
  ));
  const list = await screen.findByRole("listbox", { name: "Commit history" });
  await vi.waitFor(() => expect(selectedId()).toBe("e5"));
  list.focus();
  return { list, selectedId, setDetailsId };
}

/** The option the list says is selected. */
function active(list: HTMLElement) {
  return document.getElementById(list.getAttribute("aria-activedescendant") ?? "");
}

describe("moving through the history with the keyboard", () => {
  beforeEach(() => {
    // jsdom has no layout: give the history room for three rows below its header.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(4 * 38 + 500);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("selects the next and the previous row with the arrow keys", async () => {
    const user = userEvent.setup();
    const { list, selectedId } = await renderHistory();
    expect(active(list)).toHaveTextContent("Commit e5");

    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(selectedId()).toBe(SHAS[2]);
    expect(active(list)).toHaveTextContent("Commit c3");
    expect(active(list)).toHaveAttribute("aria-selected", "true");

    await user.keyboard("{ArrowUp}");
    expect(selectedId()).toBe(SHAS[1]);
    // The list keeps the focus, whichever row is selected.
    expect(list).toHaveFocus();
  });

  it("only loads the files of the row the details show, not of every row passed", async () => {
    rpc.fileCalls.length = 0;
    const user = userEvent.setup();
    const { selectedId, setDetailsId } = await renderHistory();
    setDetailsId("e5");
    await vi.waitFor(() => expect(rpc.fileCalls).toEqual(["e5"]));

    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowDown}");
    expect(selectedId()).toBe("b2");
    expect(rpc.fileCalls).toEqual(["e5"]);

    setDetailsId("b2");
    await vi.waitFor(() => expect(rpc.fileCalls).toEqual(["e5", "b2"]));
  });

  it("stops at the first and the last row", async () => {
    const user = userEvent.setup();
    const { selectedId } = await renderHistory();

    await user.keyboard("{ArrowUp}");
    expect(selectedId()).toBe("e5");
    await user.keyboard("{End}");
    expect(selectedId()).toBe("a1");
    await user.keyboard("{ArrowDown}");
    expect(selectedId()).toBe("a1");
    await user.keyboard("{Home}");
    expect(selectedId()).toBe("e5");
  });

  it("moves a page at a time with Page Down and Page Up", async () => {
    const user = userEvent.setup();
    const { selectedId } = await renderHistory();

    // Four rows fit in view, so a page is three.
    await user.keyboard("{PageDown}");
    expect(selectedId()).toBe("b2");
    await user.keyboard("{PageUp}");
    expect(selectedId()).toBe("e5");
  });

  it("starts at the first row that's on show when the search hides the selected one", async () => {
    const user = userEvent.setup();
    const { list, selectedId } = await renderHistory("c3");
    expect(active(list)).toBeNull();

    await user.keyboard("{ArrowDown}");
    expect(selectedId()).toBe("c3");
  });

  it("hands the focus to the list when a row is clicked", async () => {
    const user = userEvent.setup();
    const { list, selectedId } = await renderHistory();

    await user.click(screen.getByText("Commit b2"));
    expect(selectedId()).toBe("b2");
    expect(list).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(selectedId()).toBe("a1");
  });
});
