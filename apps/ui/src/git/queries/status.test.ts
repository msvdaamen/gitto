import type { Uncommitted } from "@gitto/git/types";
import { renderHook } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, type JSX } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "./keys";
import { useStatus, useUncommittedFiles } from "./status";

const rpc = vi.hoisted(() => ({ git: { status: { get: vi.fn() } } }));

vi.mock("@/lib/rpc", () => ({ rpc }));

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 1, staged: 0, unstaged: 1, conflicted: 0 },
  changes: {
    staged: [],
    unstaged: [{ path: "a.txt", status: "modified", origPath: null, additions: 1, deletions: 0 }],
    uncounted: false,
    markerFree: [],
  },
  version: "v1",
};

function render<T>(hook: () => T) {
  const client = new QueryClient();
  const { result } = renderHook(hook, {
    wrapper: (props: { children: JSX.Element }) =>
      createComponent(QueryClientProvider, {
        client,
        get children() {
          return props.children;
        },
      }),
  });
  return { result, client };
}

afterEach(() => {
  vi.resetAllMocks();
});

describe("the uncommitted changes", () => {
  it("are only sent again when they changed", async () => {
    rpc.git.status.get.mockResolvedValueOnce(status);
    const { result, client } = render(() => useUncommittedFiles(() => "repo"));
    await vi.waitFor(() => expect(result.data?.value).toEqual(status.changes));
    const file = result.data!.value.unstaged[0];
    const cached = client.getQueryData(gitKeys.status("repo"));

    rpc.git.status.get.mockResolvedValueOnce({ unchanged: true });
    await client.refetchQueries({ queryKey: gitKeys.status("repo") });
    expect(rpc.git.status.get).toHaveBeenLastCalledWith(
      { repositoryId: "repo", since: "v1" },
      expect.anything(),
    );
    expect(client.getQueryData(gitKeys.status("repo"))).toBe(cached);
    expect(result.data!.value.unstaged[0]).toBe(file);
  });

  it("are kept as they were loaded, not wrapped to track each file", async () => {
    rpc.git.status.get.mockResolvedValueOnce(status);
    const { result } = render(() => useUncommittedFiles(() => "repo"));
    await vi.waitFor(() => expect(result.data).toBeDefined());
    expect(result.data!.value).toBe(status.changes);
    expect(result.data!.value.unstaged[0]).toBe(status.changes.unstaged[0]);
  });

  it("are left out of the summary", async () => {
    rpc.git.status.get.mockResolvedValueOnce(status);
    const { result } = render(() => useStatus(() => "repo"));
    await vi.waitFor(() => expect(result.data).toBeDefined());
    expect(result.data).toEqual({
      head: status.head,
      upstream: null,
      ahead: 0,
      behind: 0,
      counts: status.counts,
    });
  });
});
