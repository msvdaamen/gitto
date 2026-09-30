import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { describe, expect, it, vi } from "vitest";

import { CommitDetails } from "./commit-details";
import { HistoryTable } from "./history-table";

const rpc = vi.hoisted(() => {
  let statusCalls = 0;
  return {
    statusCalls: () => statusCalls,
    git: {
      history: {
        log: async () => [commit("b1", "Second", ["a1"]), commit("a1", "First", [])],
        commit: async ({ sha }: { sha: string }) =>
          sha === "a1" ? commit("a1", "First", []) : commit("b1", "Second", ["a1"]),
      },
      status: {
        get: async () => {
          // Fails instead of hanging the test if the status is refetched in a loop.
          if (++statusCalls > 20) throw new Error("Status refetched in a loop");
          return {
            head: { kind: "branch", name: "main", sha: "b1" },
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [{ path: "wip.txt", origPath: null, staged: null, unstaged: "modified" }],
          };
        },
      },
      diff: {
        commitFiles: async () => [
          { path: "first.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
        ],
        workingTreeFiles: async () => ({
          staged: [],
          unstaged: [
            { path: "wip.txt", status: "modified", origPath: null, additions: 1, deletions: 0 },
          ],
        }),
      },
    },
  };
});

vi.mock("@/lib/rpc", () => ({ rpc }));

// A function declaration, so it's hoisted above `vi.hoisted` too.
function commit(sha: string, subject: string, parents: string[]) {
  return {
    sha,
    parents,
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authoredAt: 0,
    refs: [],
    subject,
    body: "",
  };
}

describe("selecting a commit", () => {
  it("shows an older commit's details while there are uncommitted changes", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const [selectedId, setSelectedId] = createSignal<string>();
    render(() => (
      <QueryClientProvider client={client}>
        <HistoryTable
          repositoryId="repo"
          search=""
          selectedId={selectedId()}
          onSelect={setSelectedId}
        />
        <CommitDetails repositoryId="repo" selectedId={selectedId()} />
      </QueryClientProvider>
    ));

    expect(await screen.findByText("Working directory")).toBeInTheDocument();
    await userEvent.click(screen.getByText("First"));

    expect(await screen.findByText("first.txt")).toBeInTheDocument();
    expect(screen.getByText("Commit details")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "First" })).toBeInTheDocument();
    expect(selectedId()).toBe("a1");
    expect(rpc.statusCalls()).toBeLessThan(20);
  });
});
