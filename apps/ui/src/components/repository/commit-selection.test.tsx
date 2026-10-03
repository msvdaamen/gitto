import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";

import { CommitDetails } from "./details/commit-details";
import { HistoryTable } from "./history/history-table";

const rpc = vi.hoisted(() => {
  let statusCalls = 0;
  return {
    statusCalls: () => statusCalls,
    git: {
      history: {
        log: async () => ({
          commits: [commit("b1", "Second", ["a1"]), commit("a1", "First", [])],
          version: "v1",
        }),
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
            counts: { files: 1, staged: 0, unstaged: 1, conflicted: 0 },
            changes: {
              staged: [],
              unstaged: [
                { path: "wip.txt", status: "modified", origPath: null, additions: 1, deletions: 0 },
              ],
            },
            version: "1",
          };
        },
      },
      diff: {
        // The second commit's files never load.
        commitFiles: ({ sha }: { sha: string }) =>
          sha === "a1"
            ? Promise.resolve([
                { path: "first.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
              ])
            : new Promise(() => undefined),
      },
      stash: {
        // Each is listed just above the commit it was made on.
        list: async () => [
          { sha: "d".repeat(40), base: "b1", message: "On main: newer stash", createdAt: 6000 },
          { sha: "c".repeat(40), base: "a1", message: "WIP on main: a1 First", createdAt: 5000 },
        ],
        // The newer stash's files never load.
        files: ({ sha }: { sha: string }) =>
          sha === "c".repeat(40)
            ? Promise.resolve([
                {
                  path: "stashed.txt",
                  status: "added",
                  origPath: null,
                  additions: 2,
                  deletions: 0,
                },
              ])
            : new Promise(() => undefined),
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
    committedAt: sha === "b1" ? 2000 : 1000,
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
  });

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
    // Only the rows in view are rendered, so each says where it is in the whole list.
    const options = await screen.findAllByRole("option");
    expect(options.map((option) => option.getAttribute("aria-posinset"))).toEqual([
      "1",
      "2",
      "3",
      "4",
      "5",
    ]);
    expect(options.every((option) => option.getAttribute("aria-setsize") === "5")).toBe(true);
    expect(options.map((option) => option.textContent)).toEqual([
      expect.stringContaining("Uncommitted changes"),
      expect.stringContaining("On main: newer stash"),
      expect.stringContaining("Second"),
      expect.stringContaining("WIP on main: a1 First"),
      expect.stringContaining("First"),
    ]);
    await userEvent.click(screen.getByText("First"));

    expect(await screen.findByText("first.txt")).toBeInTheDocument();
    expect(screen.getByText("Commit details")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "First" })).toBeInTheDocument();
    expect(selectedId()).toBe("a1");
    expect(rpc.statusCalls()).toBeLessThan(20);

    // While another commit's files load, the message stays the one whose files are shown.
    await userEvent.click(screen.getByText("Second"));
    expect(selectedId()).toBe("b1");
    await vi.waitFor(() =>
      expect(client.getQueryState(gitKeys.commit("repo", "b1"))?.status).toBe("success"),
    );
    expect(screen.getByRole("heading", { name: "First" })).toBeInTheDocument();
    expect(screen.getByText("first.txt")).toBeInTheDocument();
  });

  it("shows the history while the stashes are still loading", async () => {
    const list = rpc.git.stash.list;
    rpc.git.stash.list = () => new Promise<never>(() => undefined);
    try {
      const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      render(() => (
        <QueryClientProvider client={client}>
          <HistoryTable repositoryId="repo" search="" selectedId={undefined} onSelect={() => {}} />
        </QueryClientProvider>
      ));

      expect(await screen.findByText("Second")).toBeInTheDocument();
      expect(screen.queryByText("On main: newer stash")).not.toBeInTheDocument();
    } finally {
      rpc.git.stash.list = list;
    }
  });

  it("shows a stash's details, with the files it changed", async () => {
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

    await userEvent.click(await screen.findByText("WIP on main: a1 First"));
    expect(await screen.findByText("stashed.txt")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "WIP on main: a1 First" })).toBeInTheDocument();
    expect(selectedId()).toBe(`stash:${"c".repeat(40)}`);

    // While another stash's files load, the summary still names the one whose files are shown.
    await userEvent.click(screen.getByText("On main: newer stash"));
    expect(selectedId()).toBe(`stash:${"d".repeat(40)}`);
    expect(screen.getByRole("heading", { name: "WIP on main: a1 First" })).toBeInTheDocument();
    expect(screen.getByText("stashed.txt")).toBeInTheDocument();
  });
});
