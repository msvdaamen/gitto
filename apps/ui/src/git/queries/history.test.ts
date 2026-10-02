import type { Commit } from "@gitto/git/types";
import { renderHook } from "@solidjs/testing-library";
import { QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, type JSX } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createQueryClient } from "@/lib/query-client";

import { useHistory } from "./history";
import { gitKeys } from "./keys";

const rpc = vi.hoisted(() => ({ git: { history: { log: vi.fn() }, status: { get: vi.fn() } } }));

vi.mock("@/lib/rpc", () => ({ rpc }));

/** A straight history of 2500 commits, newest first. */
const commits: Commit[] = Array.from({ length: 2500 }, (_, index) => ({
  sha: `sha${index}`,
  parents: index < 2499 ? [`sha${index + 1}`] : [],
  authorName: "Ada Lovelace",
  authorEmail: "ada@example.com",
  authoredAt: 0,
  committedAt: 0,
  refs: [],
  subject: `Commit ${index}`,
  body: "",
}));

function render() {
  const client = createQueryClient();
  const { result } = renderHook(
    () =>
      useHistory(
        () => "repo",
        () => undefined,
      ),
    {
      wrapper: (props: { children: JSX.Element }) =>
        createComponent(QueryClientProvider, {
          client,
          get children() {
            return props.children;
          },
        }),
    },
  );
  return { history: result, client };
}

beforeEach(() => {
  rpc.git.status.get.mockResolvedValue({
    head: { kind: "branch", name: "main", sha: "sha0" },
    upstream: null,
    ahead: 0,
    behind: 0,
    counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
    changes: { staged: [], unstaged: [] },
    version: "v1",
  });
  rpc.git.history.log.mockImplementation(
    async ({ limit, skip = 0, since }: { limit: number; skip?: number; since?: string }) => {
      const version = `${skip}+${limit}`;
      if (since === version) return { unchanged: true };
      // New objects every time, like commits that come in over RPC.
      return { commits: structuredClone(commits.slice(skip, skip + limit)), version };
    },
  );
});

afterEach(() => {
  vi.resetAllMocks();
});

describe("the history", () => {
  it("loads its newest commits first, and older ones when asked", async () => {
    const { history } = render();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(200));
    expect(history.complete()).toBe(false);
    const first = history.rows()[0];

    await history.loadMore();
    expect(rpc.git.history.log).toHaveBeenLastCalledWith({
      repositoryId: "repo",
      limit: 1000,
      skip: 200,
    });
    await vi.waitFor(() => expect(history.rows()).toHaveLength(1200));
    // The rows that were there are kept.
    expect(history.rows()[0]).toBe(first);
    expect(history.rows()[1199]).toMatchObject({ message: "Commit 1199" });

    await history.loadMore();
    await history.loadMore();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(2500));
    expect(history.complete()).toBe(true);

    // Nothing left to load.
    rpc.git.history.log.mockClear();
    await history.loadMore();
    expect(rpc.git.history.log).not.toHaveBeenCalled();
  });

  it("reloads as many commits as were loaded, in one go, and only when they changed", async () => {
    const { history, client } = render();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(200));
    await history.loadMore();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(1200));

    // E.g. after a commit: all 1200 at once, not known to be the same as before.
    await client.invalidateQueries({ queryKey: gitKeys.log("repo") });
    expect(rpc.git.history.log).toHaveBeenLastCalledWith(
      { repositoryId: "repo", limit: 1200, since: undefined },
      expect.anything(),
    );
    await vi.waitFor(() => expect(history.rows()[0]).toMatchObject({ message: "Commit 0" }));
    expect(history.rows()).toHaveLength(1200);
    const first = history.rows()[0];

    // Nothing changed since: the commits aren't sent, and the rows stay as they are.
    await client.invalidateQueries({ queryKey: gitKeys.log("repo") });
    expect(rpc.git.history.log).toHaveBeenLastCalledWith(
      { repositoryId: "repo", limit: 1200, since: "0+1200" },
      expect.anything(),
    );
    expect(history.rows()[0]).toBe(first);
  });

  it("keeps older commits added while it was reloading", async () => {
    const { history, client } = render();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(200));

    // The reload takes a while (a big repository), and more is loaded meanwhile.
    const reload = Promise.withResolvers<void>();
    const answer = rpc.git.history.log.getMockImplementation()!;
    rpc.git.history.log.mockImplementation(async (input: { skip?: number }) => {
      if (!input.skip) await reload.promise;
      return answer(input);
    });
    const reloading = client.invalidateQueries({ queryKey: gitKeys.log("repo") });
    await history.loadMore();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(1200));

    reload.resolve();
    await reloading;
    expect(history.rows()).toHaveLength(1200);
  });

  it("doesn't load more by itself after it failed, until asked", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const { history } = render();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(200));

    const answer = rpc.git.history.log.getMockImplementation()!;
    rpc.git.history.log.mockImplementation(async (input: { skip?: number }) => {
      if (input.skip) throw new Error("bad object");
      return answer(input);
    });
    await history.loadMore();
    expect(logged).toHaveBeenCalledOnce();
    expect(history.loadFailed()).toBe(true);
    expect(history.rows()).toHaveLength(200);

    rpc.git.history.log.mockImplementation(answer);
    await history.loadMore();
    expect(history.loadFailed()).toBe(false);
    await vi.waitFor(() => expect(history.rows()).toHaveLength(1200));
    logged.mockRestore();
  });

  it("doesn't add older commits to a log that's to be reloaded", async () => {
    const { history, client } = render();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(200));

    // Like leaving the repository while more of it loads: no longer watched, so out of date.
    const loading = history.loadMore();
    await client.invalidateQueries({ queryKey: gitKeys.log("repo"), refetchType: "none" });
    await loading;
    expect(history.rows()).toHaveLength(200);
    expect(client.getQueryState(gitKeys.log("repo"))?.isInvalidated).toBe(true);
  });

  it("doesn't add older commits to a log that was reloaded meanwhile", async () => {
    const { history, client } = render();
    await vi.waitFor(() => expect(history.rows()).toHaveLength(200));

    // The older commits take longer to come than the reload, which has a new commit on top: the
    // page asked for (by position) may start with one it has already, or skip one.
    const older = Promise.withResolvers<void>();
    const answer = rpc.git.history.log.getMockImplementation()!;
    rpc.git.history.log.mockImplementation(async (input: { skip?: number; limit: number }) => {
      if (input.skip) {
        await older.promise;
        return answer(input);
      }
      const newest = { ...commits[0]!, sha: "new", parents: ["sha0"], subject: "New" };
      return { commits: [newest, ...commits.slice(0, input.limit - 1)], version: "new" };
    });
    const loading = history.loadMore();
    await client.invalidateQueries({ queryKey: gitKeys.log("repo") });
    await vi.waitFor(() => expect(history.rows()[0]).toMatchObject({ message: "New" }));
    older.resolve();
    await loading;
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(history.rows()).toHaveLength(200);
    expect(history.loadingMore()).toBe(false);
  });
});
