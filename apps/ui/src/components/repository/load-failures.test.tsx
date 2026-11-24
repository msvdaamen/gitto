import { render, screen } from "@solidjs/testing-library";
import type { JSX } from "@solidjs/web";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createSignal, Errored } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BranchMenuProvider } from "./branch-menu";
import { CommitDetails } from "./details/commit-details";
import { HistoryTable } from "./history/history-table";

const rpc = vi.hoisted(() => ({
  repository: { list: async () => [{ id: "repo", name: "repo", path: "/w/repo" }] },
  git: {
    history: { log: vi.fn(), commit: vi.fn() },
    status: { get: vi.fn() },
    diff: { commitFiles: vi.fn() },
    stash: { list: async () => [] },
    refs: { list: async () => [] },
    operation: { get: async () => null },
    worktree: { list: async () => [] },
  },
}));

vi.mock("@/lib/rpc", () => ({ rpc }));

const commit = {
  sha: "e5",
  parents: [],
  authorName: "Ada Lovelace",
  authorEmail: "ada@example.com",
  authoredAt: 0,
  committedAt: 0,
  refs: [],
  subject: "Commit e5",
  body: "",
};

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(700);
  rpc.git.status.get.mockResolvedValue({
    head: { kind: "branch", name: "main", sha: "e5" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    changes: { staged: [], unstaged: [], uncounted: false, markerFree: [] },
    version: "1",
  });
  rpc.git.history.log.mockResolvedValue([commit]);
  rpc.git.history.commit.mockResolvedValue(commit);
});

afterEach(() => {
  vi.resetAllMocks();
  vi.restoreAllMocks();
});

/**
 * What the app has around a page: a query client, and the router's boundary, which a failed load
 * reports to, and which replaces the page if anything on it throws that failure on.
 */
function Page(props: { children: JSX.Element }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={client}>
      <Errored fallback="Something went wrong">{props.children}</Errored>
    </QueryClientProvider>
  );
}

describe("a load that fails", () => {
  it("says why the history couldn't be loaded", async () => {
    rpc.git.history.log.mockRejectedValue(new Error("Not a git repository."));
    const [selectedId, setSelectedId] = createSignal<string>();
    render(() => (
      <Page>
        <BranchMenuProvider repositoryId="repo">
          <HistoryTable
            repositoryId="repo"
            search=""
            selectedId={selectedId()}
            detailsId={undefined}
            onSelect={setSelectedId}
          />
        </BranchMenuProvider>
      </Page>
    ));

    expect(await screen.findByText("Couldn't load history")).toBeInTheDocument();
    expect(screen.getByText("Not a git repository.")).toBeInTheDocument();
    expect(screen.queryByText("Something went wrong")).not.toBeInTheDocument();
  });

  it("says why a commit's files couldn't be loaded", async () => {
    rpc.git.diff.commitFiles.mockRejectedValue(new Error("Bad object e5."));
    render(() => (
      <Page>
        <CommitDetails repositoryId="repo" selectedId="e5" />
      </Page>
    ));

    expect(await screen.findByText("Couldn't load the commit")).toBeInTheDocument();
    expect(screen.getByText("Bad object e5.")).toBeInTheDocument();
    expect(screen.queryByText("Something went wrong")).not.toBeInTheDocument();
  });
});
