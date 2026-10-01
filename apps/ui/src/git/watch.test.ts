import { renderHook } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, type JSX } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "./keys";
import { useRepositoryWatcher } from "./watch";

const rpc = vi.hoisted(() => {
  const gitDir = procedure<("index" | "refs")[]>();
  const workingTree = procedure<null>();
  return {
    gitDir,
    workingTree,
    git: { watch: { gitDir: gitDir.call, workingTree: workingTree.call } },
  };
});

vi.mock("@/lib/rpc", () => ({ rpc }));

function watch(focused: boolean) {
  vi.spyOn(document, "hasFocus").mockReturnValue(focused);
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const { cleanup } = renderHook(() => useRepositoryWatcher(() => "repo"), {
    wrapper: (props: { children: JSX.Element }) =>
      createComponent(QueryClientProvider, {
        client,
        get children() {
          return props.children;
        },
      }),
  });
  const invalidated = () => invalidate.mock.calls.map(([filters]) => filters?.queryKey);
  return { invalidate, invalidated, cleanup };
}

afterEach(() => vi.restoreAllMocks());

/**
 * A watch procedure whose events the test pushes; counts how many subscriptions are open. A
 * function declaration, so it's hoisted above `vi.hoisted` too.
 */
function procedure<T>() {
  let push: ((event: T) => void) | undefined;
  const state = {
    open: 0,
    push: (event: T) => push?.(event),
    call: async (_input: unknown, { signal }: { signal: AbortSignal }) => {
      state.open++;
      signal.addEventListener("abort", () => state.open--);
      return {
        [Symbol.asyncIterator]: () => ({
          next: () =>
            new Promise<IteratorResult<T>>((resolve) => {
              push = (value) => resolve({ value, done: false });
            }),
        }),
      };
    },
  };
  return state;
}

describe("watching a repository", () => {
  it("refetches only what a change in the git directory affects", async () => {
    const { invalidate, invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.gitDir.open).toBe(1));

    rpc.gitDir.push(["index"]);
    await vi.waitFor(() => expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]));

    invalidate.mockClear();
    rpc.gitDir.push(["index", "refs"]);
    await vi.waitFor(() => expect(invalidated()).toEqual([gitKeys.repository("repo")]));

    cleanup();
    expect(rpc.gitDir.open).toBe(0);
  });

  it("watches the working tree only while the window has focus", async () => {
    const { invalidate, invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));
    expect(invalidated()).toEqual([]);

    rpc.workingTree.push(null);
    await vi.waitFor(() => expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]));

    window.dispatchEvent(new Event("blur"));
    expect(rpc.workingTree.open).toBe(0);
    expect(rpc.gitDir.open).toBe(1);

    // Catches up on what changed in the meantime.
    invalidate.mockClear();
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    cleanup();
    expect(rpc.workingTree.open).toBe(0);
  });

  it("starts watching the working tree once the window gets focus", async () => {
    const { invalidated, cleanup } = watch(false);
    await vi.waitFor(() => expect(rpc.gitDir.open).toBe(1));
    expect(rpc.workingTree.open).toBe(0);

    window.dispatchEvent(new Event("focus"));
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);

    cleanup();
  });
});
