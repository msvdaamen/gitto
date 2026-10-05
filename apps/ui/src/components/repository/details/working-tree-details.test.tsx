import type { ChangedFile, Uncommitted } from "@gitto/git/types";
import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { Suspense } from "solid-js";
import { describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";

import { WorkingTreeDetails } from "./working-tree-details";

const rpc = vi.hoisted(() => ({
  git: {
    status: { get: vi.fn(async () => uncommitted) },
    commit: { message: vi.fn(), pushedTo: vi.fn() },
  },
}));
vi.mock("@/lib/rpc", () => ({ rpc }));

function file(path: string): ChangedFile {
  return { path, status: "modified", origPath: null, additions: 1, deletions: 0 };
}

let uncommitted: Uncommitted;
function setChanges(unstaged: ChangedFile[]) {
  uncommitted = {
    head: { kind: "branch", name: "main", sha: "a1" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: unstaged.length, staged: 0, unstaged: unstaged.length, conflicted: 0 },
    changes: { staged: [], unstaged, uncounted: false },
    version: String(Math.random()),
  };
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
