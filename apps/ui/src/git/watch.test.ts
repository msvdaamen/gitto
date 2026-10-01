import { renderHook } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, type JSX } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "./keys";
import { POLL_MS, useRepositoryWatcher } from "./watch";

const rpc = vi.hoisted(() => {
  const gitDir = procedure<("index" | "refs")[]>();
  return { gitDir, git: { watch: { gitDir: gitDir.call } } };
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

beforeEach(() => vi.useFakeTimers());

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

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
    const { invalidate, invalidated, cleanup } = watch(false);
    await vi.waitFor(() => expect(rpc.gitDir.open).toBe(1));

    rpc.gitDir.push(["index"]);
    await vi.waitFor(() => expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]));

    invalidate.mockClear();
    rpc.gitDir.push(["index", "refs"]);
    await vi.waitFor(() => expect(invalidated()).toEqual([gitKeys.repository("repo")]));

    cleanup();
    expect(rpc.gitDir.open).toBe(0);
  });

  it("polls the uncommitted changes only while the window has focus", async () => {
    const { invalidate, invalidated, cleanup } = watch(true);
    expect(invalidated()).toEqual([]);

    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(invalidated()).toHaveLength(2);

    window.dispatchEvent(new Event("blur"));
    invalidate.mockClear();
    await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    expect(invalidated()).toEqual([]);

    // Catches up on what changed in the meantime, then polls again.
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(invalidated()).toHaveLength(2);

    cleanup();
    invalidate.mockClear();
    await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    expect(invalidated()).toEqual([]);
  });

  it("starts polling once the window gets focus", async () => {
    const { invalidated, cleanup } = watch(false);
    await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    expect(invalidated()).toEqual([]);

    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(invalidated()).toHaveLength(2);

    cleanup();
  });
});
