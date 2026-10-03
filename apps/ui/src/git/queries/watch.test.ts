import { renderHook } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider, QueryObserver } from "@tanstack/solid-query";
import { createComponent, type JSX } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { gitKeys } from "./keys";
import { UNWATCH_AFTER_MS, useRepositoryWatcher } from "./watch";

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
  return { client, invalidate, invalidated, cleanup };
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
  let fail: ((error: Error) => void) | undefined;
  let end: (() => void) | undefined;
  const state = {
    open: 0,
    push: (event: T) => push?.(event),
    /** Ends the open subscription with an error, like the watcher running out of file watches. */
    fail: (error: Error) => fail?.(error),
    /** Ends the open subscription normally, like the server closing it. */
    end: () => end?.(),
    call: async (_input: unknown, { signal }: { signal: AbortSignal }) => {
      state.open++;
      signal.addEventListener("abort", () => state.open--);
      return {
        [Symbol.asyncIterator]: () => ({
          next: () =>
            new Promise<IteratorResult<T>>((resolve, reject) => {
              push = (value) => resolve({ value, done: false });
              fail = (error) => {
                state.open--;
                reject(error);
              };
              end = () => {
                state.open--;
                resolve({ value: undefined, done: true });
              };
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

  it("lets a refetch that's running finish, then refetches it again", async () => {
    const { client, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.gitDir.open).toBe(1));
    const loads: { signal: AbortSignal; resolve: (log: string) => void }[] = [];
    const log = new QueryObserver(client, {
      queryKey: gitKeys.log("repo"),
      queryFn: ({ signal }) => new Promise<string>((resolve) => loads.push({ signal, resolve })),
    });
    const unsubscribe = log.subscribe(() => undefined);
    await vi.waitFor(() => expect(loads).toHaveLength(1));
    loads[0]!.resolve("first");
    await vi.waitFor(() => expect(log.getCurrentResult().data).toBe("first"));

    // Refetched (by a commit in Gitto, say), and reported by the watcher while that runs.
    void client.invalidateQueries({ queryKey: gitKeys.log("repo") });
    await vi.waitFor(() => expect(loads).toHaveLength(2));
    rpc.gitDir.push(["refs"]);
    await vi.advanceTimersByTimeAsync(0);
    expect(loads).toHaveLength(2);
    expect(loads[1]!.signal.aborted).toBe(false);

    loads[1]!.resolve("second");
    await vi.waitFor(() => expect(loads).toHaveLength(3));
    loads[2]!.resolve("third");
    await vi.waitFor(() => expect(log.getCurrentResult().data).toBe("third"));

    unsubscribe();
    cleanup();
  });

  it("refetches changes to the working tree right away while the window has focus", async () => {
    const { invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));
    expect(invalidated()).toEqual([]);

    rpc.workingTree.push(null);
    await vi.waitFor(() => expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]));

    cleanup();
    expect(rpc.workingTree.open).toBe(0);
  });

  it("keeps watching for a while after losing focus, and only refetches what changed", async () => {
    const { invalidate, invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    // Nothing changed: nothing to refetch, and no new watch.
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([]);
    expect(rpc.workingTree.open).toBe(1);

    // Changes while unfocused are refetched once, when the window gets focus back.
    window.dispatchEvent(new Event("blur"));
    rpc.workingTree.push(null);
    await Promise.resolve();
    rpc.workingTree.push(null);
    await vi.advanceTimersByTimeAsync(UNWATCH_AFTER_MS - 1);
    expect(invalidated()).toEqual([]);
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);

    invalidate.mockClear();
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([]);

    cleanup();
  });

  it("stops watching once the window lost focus long enough, and catches up after", async () => {
    const { invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    window.dispatchEvent(new Event("blur"));
    await vi.advanceTimersByTimeAsync(UNWATCH_AFTER_MS);
    expect(rpc.workingTree.open).toBe(0);
    expect(rpc.gitDir.open).toBe(1);

    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    cleanup();
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

  it("tries again on the next focus when watching fails", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const { invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    rpc.workingTree.fail(new Error("no space left on device"));
    await vi.waitFor(() => expect(logged).toHaveBeenCalled());
    expect(rpc.workingTree.open).toBe(0);

    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    cleanup();
  });

  it("starts watching the working tree again on the next focus when its stream ended", async () => {
    const { invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    rpc.workingTree.end();
    await vi.advanceTimersByTimeAsync(0);
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.uncommitted("repo")]);
    await vi.waitFor(() => expect(rpc.workingTree.open).toBe(1));

    cleanup();
  });

  it("watches the git directory again, and refetches everything, when it failed", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const { invalidated, cleanup } = watch(true);
    await vi.waitFor(() => expect(rpc.gitDir.open).toBe(1));

    rpc.gitDir.fail(new Error("no space left on device"));
    await vi.waitFor(() => expect(logged).toHaveBeenCalled());
    expect(rpc.gitDir.open).toBe(0);

    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("focus"));
    expect(invalidated()).toEqual([gitKeys.repository("repo")]);
    await vi.waitFor(() => expect(rpc.gitDir.open).toBe(1));

    cleanup();
  });
});
