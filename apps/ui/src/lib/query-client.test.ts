import type { Uncommitted } from "@gitto/git/types";
import { renderHook } from "@solidjs/testing-library";
import { QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, type JSX } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "@/git/queries/keys";
import { useStatus } from "@/git/queries/status";

import { queryClient } from "./query-client";

const rpc = vi.hoisted(() => ({ git: { status: { get: vi.fn() } } }));

vi.mock("@/lib/rpc", () => ({ rpc }));

const status: Uncommitted = {
  head: { kind: "branch", name: "main", sha: "abc" },
  upstream: null,
  ahead: 0,
  behind: 0,
  counts: { files: 0, staged: 0, unstaged: 0, conflicted: 0 },
  changes: { staged: [], unstaged: [] },
  version: "v1",
};

/** Renders the status the way a component showing it does. */
function renderStatus() {
  return renderHook(() => useStatus(() => "repo"), {
    wrapper: (props: { children: JSX.Element }) =>
      createComponent(QueryClientProvider, {
        client: queryClient,
        get children() {
          return props.children;
        },
      }),
  });
}

afterEach(() => {
  queryClient.clear();
  vi.resetAllMocks();
});

describe("a repository's data", () => {
  it("isn't loaded again when something else that shows it is rendered", async () => {
    rpc.git.status.get.mockResolvedValue(status);
    const first = renderStatus();
    await vi.waitFor(() => expect(first.result.data).toBeDefined());

    const second = renderStatus();
    await vi.waitFor(() => expect(second.result.data).toBeDefined());
    expect(rpc.git.status.get).toHaveBeenCalledTimes(1);
  });

  it("is loaded again once it's said to have changed, even if nothing showed it then", async () => {
    rpc.git.status.get.mockResolvedValue(status);
    const first = renderStatus();
    await vi.waitFor(() => expect(first.result.data).toBeDefined());
    first.cleanup();

    await queryClient.invalidateQueries({ queryKey: gitKeys.repository("repo") });
    expect(rpc.git.status.get).toHaveBeenCalledTimes(1);
    const second = renderStatus();
    await vi.waitFor(() => expect(rpc.git.status.get).toHaveBeenCalledTimes(2));
    second.cleanup();
  });
});
