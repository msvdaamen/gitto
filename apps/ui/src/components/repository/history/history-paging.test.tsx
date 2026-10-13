import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BranchMenuProvider } from "../branch-menu";
import { ROW_HEIGHT } from "./columns";
import { HistoryTable } from "./history-table";

/** How many commits the repository has: more than the first two pages. */
const COMMITS = 1500;

const rpc = vi.hoisted(() => ({
  git: {
    history: { log: vi.fn() },
    status: {
      get: async () => ({
        head: { kind: "branch", name: "main", sha: "c0" },
        upstream: null,
        ahead: 0,
        behind: 0,
        counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
        changes: { staged: [], unstaged: [], uncounted: false },
        version: "1",
      }),
    },
    diff: { commitFiles: async () => [] },
    stash: { list: async () => [] },
    worktree: { list: async () => [] },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** Commit `index` of a line of `COMMITS`, newest first. */
function commit(index: number) {
  return {
    sha: `c${index}`,
    parents: index + 1 < COMMITS ? [`c${index + 1}`] : [],
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: 0,
    committedAt: (COMMITS - index) * 1000,
    refs: [],
    subject: `Commit ${index}`,
    body: "",
  };
}

let scrollTop = 0;

beforeEach(() => {
  // jsdom has no layout: give the history room for ten rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(10 * ROW_HEIGHT);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(10 * ROW_HEIGHT);
  vi.spyOn(HTMLElement.prototype, "scrollTop", "get").mockImplementation(() => scrollTop);
  scrollTop = 0;
  rpc.git.history.log.mockImplementation(async ({ skip, limit }: { skip: number; limit: number }) =>
    Array.from({ length: Math.max(0, Math.min(limit, COMMITS - skip)) }, (_, i) =>
      commit(skip + i),
    ),
  );
});

afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
});

async function renderHistory(search = "") {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const [selectedId, setSelectedId] = createSignal<string>();
  render(() => (
    <QueryClientProvider client={client}>
      <BranchMenuProvider repositoryId="repo">
        <HistoryTable
          repositoryId="repo"
          search={search}
          selectedId={selectedId()}
          detailsId={undefined}
          onSelect={setSelectedId}
        />
      </BranchMenuProvider>
    </QueryClientProvider>
  ));
  await screen.findAllByRole("option");
  return screen.getByRole("main");
}

/** Scrolls the history so that row `index` is the first in view. */
function scrollTo(scroller: HTMLElement, index: number) {
  scrollTop = index * ROW_HEIGHT;
  scroller.dispatchEvent(new Event("scroll"));
}

/** The pages of the log asked for so far. */
function pages() {
  return rpc.git.history.log.mock.calls.map(([{ skip, limit }]) => ({ skip, limit }));
}

describe("the history", () => {
  it("loads more as its end comes near, until there's none left", async () => {
    const scroller = await renderHistory();
    expect(pages()).toEqual([{ skip: 0, limit: 200 }]);

    // Well before the end of the first page.
    scrollTo(scroller, 50);
    await screen.findByText("Commit 50");
    expect(pages()).toHaveLength(1);

    scrollTo(scroller, 120);
    await vi.waitFor(() =>
      expect(pages()).toEqual([
        { skip: 0, limit: 200 },
        { skip: 200, limit: 1000 },
      ]),
    );
    scrollTo(scroller, 1150);
    expect(await screen.findByText("Commit 1150")).toBeInTheDocument();
    await vi.waitFor(() => expect(pages()).toHaveLength(3));
    expect(pages()[2]).toEqual({ skip: 1200, limit: 1000 });

    // The last page was short of its limit: that's the whole history.
    scrollTo(scroller, COMMITS - 10);
    expect(await screen.findByText(`Commit ${COMMITS - 1}`)).toBeInTheDocument();
    expect(pages()).toHaveLength(3);
  });

  it("doesn't load more while searching", async () => {
    // Only a handful of commits match, so the end is in view straight away.
    await renderHistory("Commit 7");
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(pages()).toEqual([{ skip: 0, limit: 200 }]);
  });
});
