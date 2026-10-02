import type { FileChange } from "@gitto/git/types";
import { renderHook } from "@solidjs/testing-library";
import { QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, createSignal, type JSX } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createQueryClient } from "@/lib/query-client";

import { useLineCounts } from "./diff";
import { gitKeys } from "./keys";

const rpc = vi.hoisted(() => ({ git: { diff: { lineCounts: vi.fn() } } }));

vi.mock("@/lib/rpc", () => ({ rpc }));

const file = (path: string, status: FileChange["status"] = "modified"): FileChange => ({
  path,
  status,
  origPath: null,
});

function render<T>(hook: () => T) {
  const client = createQueryClient();
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

beforeEach(() => {
  vi.useFakeTimers();
  // Counts one line added per file asked for.
  rpc.git.diff.lineCounts.mockImplementation(async ({ files }: { files: FileChange[] }) =>
    files.map(({ path }) => ({ path, additions: 1, deletions: 0 })),
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.resetAllMocks();
});

describe("line counts of uncommitted changes", () => {
  it("are asked for the files on show, once scrolling pauses", async () => {
    const [visible, setVisible] = createSignal<FileChange[]>([]);
    const { result: counts } = render(() => useLineCounts(() => "repo", "unstaged", visible));
    expect(rpc.git.diff.lineCounts).not.toHaveBeenCalled();

    // The first files right away; untracked ones have nothing to count, renames need both paths.
    setVisible([
      file("a.txt"),
      file("new.txt", "untracked"),
      { ...file("moved.txt", "renamed"), origPath: "old.txt" },
    ]);
    await vi.advanceTimersByTimeAsync(0);
    expect(rpc.git.diff.lineCounts).toHaveBeenCalledExactlyOnceWith(
      {
        repositoryId: "repo",
        staged: false,
        files: [
          { path: "a.txt", origPath: null },
          { path: "moved.txt", origPath: "old.txt" },
        ],
      },
      expect.anything(),
    );
    expect(counts(file("a.txt"))).toEqual({ path: "a.txt", additions: 1, deletions: 0 });
    expect(counts(file("new.txt", "untracked"))).toBeUndefined();

    // Scrolling: only where it comes to rest is asked for.
    setVisible([file("b.txt")]);
    await vi.advanceTimersByTimeAsync(50);
    setVisible([file("c.txt")]);
    await vi.advanceTimersByTimeAsync(150);
    expect(rpc.git.diff.lineCounts).toHaveBeenCalledTimes(2);
    expect(rpc.git.diff.lineCounts).toHaveBeenLastCalledWith(
      { repositoryId: "repo", staged: false, files: [{ path: "c.txt", origPath: null }] },
      expect.anything(),
    );
    expect(counts(file("c.txt"))).toEqual({ path: "c.txt", additions: 1, deletions: 0 });
    // Files that scrolled out of view keep the counts they had.
    expect(counts(file("a.txt"))).toEqual({ path: "a.txt", additions: 1, deletions: 0 });
  });

  it("don't apply to a file that's untracked now", async () => {
    const [visible, setVisible] = createSignal([file("a.txt")]);
    const { result: counts } = render(() => useLineCounts(() => "repo", "unstaged", visible));
    await vi.advanceTimersByTimeAsync(0);
    expect(counts(file("a.txt"))).toBeDefined();

    // E.g. after `git rm --cached a.txt`.
    setVisible([file("a.txt", "untracked")]);
    await vi.advanceTimersByTimeAsync(150);
    expect(counts(file("a.txt", "untracked"))).toBeUndefined();
  });

  it("are counted again when the working tree changes", async () => {
    const [visible] = createSignal([file("a.txt"), file("b.txt")]);
    const { result: counts, client } = render(() => useLineCounts(() => "repo", "staged", visible));
    await vi.advanceTimersByTimeAsync(0);
    expect(counts(file("b.txt"))).toBeDefined();

    // `b.txt` has no staged changes anymore.
    rpc.git.diff.lineCounts.mockResolvedValueOnce([{ path: "a.txt", additions: 5, deletions: 2 }]);
    await client.invalidateQueries({ queryKey: gitKeys.uncommitted("repo") });
    await vi.advanceTimersByTimeAsync(0);
    expect(rpc.git.diff.lineCounts).toHaveBeenCalledTimes(2);
    expect(counts(file("a.txt"))).toEqual({ path: "a.txt", additions: 5, deletions: 2 });
    expect(counts(file("b.txt"))).toBeUndefined();
  });
});
