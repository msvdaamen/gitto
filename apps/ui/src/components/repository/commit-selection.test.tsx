import { render, screen } from "@solidjs/testing-library";
import { QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createQueryClient } from "@/lib/query-client";

import { CommitDetails } from "./details/commit-details";
import { HistoryTable } from "./history/history-table";

const rpc = vi.hoisted(() => {
  let statusCalls = 0;
  let commitCalls = 0;
  const hold = { status: undefined as Promise<void> | undefined };
  return {
    statusCalls: () => statusCalls,
    commitCalls: () => commitCalls,
    /** Set to a promise to keep the status from coming in until it resolves. */
    hold,
    git: {
      history: {
        log: async () => ({
          commits: [commit("b1", "Second", ["a1"]), commit("a1", "First", [])],
          version: "1",
        }),
        commit: async ({ sha }: { sha: string }) => {
          commitCalls++;
          return sha === "a1" ? commit("a1", "First", []) : commit("b1", "Second", ["a1"]);
        },
      },
      status: {
        get: async () => {
          // Fails instead of hanging the test if the status is refetched in a loop.
          if (++statusCalls > 20) throw new Error("Status refetched in a loop");
          await hold.status;
          return {
            head: { kind: "branch", name: "main", sha: "b1" },
            upstream: null,
            ahead: 0,
            behind: 0,
            counts: { files: 1, staged: 0, unstaged: 1, conflicted: 0 },
            changes: {
              staged: [],
              unstaged: [{ path: "wip.txt", status: "modified", origPath: null }],
            },
            version: "1",
          };
        },
      },
      diff: {
        lineCounts: async () => [{ path: "wip.txt", additions: 1, deletions: 0 }],
        commitFiles: async () => [
          { path: "first.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
        ],
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
    committedAt: 0,
    refs: [],
    subject,
    body: "",
  };
}

describe("selecting a commit", () => {
  beforeEach(() => {
    // jsdom has no layout: give the history and the details some room, so their rows render.
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(500);
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(500);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    rpc.hold.status = undefined;
  });

  it("shows an older commit's details while there are uncommitted changes", async () => {
    const client = createQueryClient();
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
    // Only the rows in view are rendered, so each says where it is in the whole list.
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.getAttribute("aria-posinset"))).toEqual(["1", "2", "3"]);
    expect(options.every((option) => option.getAttribute("aria-setsize") === "3")).toBe(true);
    await userEvent.click(screen.getByText("First"));

    expect(await screen.findByText("first.txt")).toBeInTheDocument();
    expect(screen.getByText("Commit details")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "First" })).toBeInTheDocument();
    expect(selectedId()).toBe("a1");
    expect(rpc.statusCalls()).toBeLessThan(20);
    // The log already had the commit, and mounting the details didn't load the status again.
    expect(rpc.commitCalls()).toBe(0);
    expect(rpc.statusCalls()).toBe(1);
  });

  it("shows the history before the status is in, and the uncommitted changes once it is", async () => {
    const held = Promise.withResolvers<void>();
    rpc.hold.status = held.promise;
    const client = createQueryClient();
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

    // The commits, and nothing selected yet: the first row may still become the uncommitted changes.
    expect(await screen.findByText("Second")).toBeInTheDocument();
    expect(screen.getAllByRole("option")).toHaveLength(2);
    expect(selectedId()).toBeUndefined();
    expect(screen.getByText("Nothing selected")).toBeInTheDocument();

    held.resolve();
    expect(await screen.findByText("Working directory")).toBeInTheDocument();
    expect(screen.getAllByRole("option")).toHaveLength(3);
    expect(selectedId()).toBe("wip");
    // The history stays on show while the uncommitted changes join it.
    expect(screen.getByText("Second")).toBeInTheDocument();
  });
});
