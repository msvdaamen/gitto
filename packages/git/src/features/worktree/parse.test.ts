import { describe, expect, it } from "vitest";

import { parseWorktreeList } from "./parse";

/** `worktree list --porcelain -z`'s output for the worktrees `entries`, each its lines. */
function output(...entries: string[][]): string {
  return entries.map((lines) => `${lines.join("\0")}\0\0`).join("");
}

describe("parseWorktreeList", () => {
  it("reads the main worktree first, then the linked ones, on a branch or detached", () => {
    expect(
      parseWorktreeList(
        output(
          ["worktree /repo", "HEAD aaa", "branch refs/heads/main"],
          ["worktree /repo-feature", "HEAD bbb", "branch refs/heads/feature"],
          ["worktree /tmp/repo-check", "HEAD ccc", "detached"],
        ),
      ),
    ).toEqual([
      {
        path: "/repo",
        name: "repo",
        head: "aaa",
        branch: "refs/heads/main",
        main: true,
        bare: false,
        current: false,
        locked: null,
        prunable: null,
      },
      expect.objectContaining({
        path: "/repo-feature",
        name: "repo-feature",
        head: "bbb",
        branch: "refs/heads/feature",
        main: false,
      }),
      expect.objectContaining({ path: "/tmp/repo-check", head: "ccc", branch: null, main: false }),
    ]);
  });

  it("reads why a worktree is locked or prunable, and a lock without a reason", () => {
    const [locked, bare, prunable] = parseWorktreeList(
      output(
        ["worktree /a", "HEAD aaa", "detached", "locked busy now"],
        ["worktree /b", "bare"],
        [
          "worktree /c",
          "HEAD ccc",
          "branch refs/heads/x",
          "locked",
          "prunable gitdir file points to non-existent location",
        ],
      ),
    );
    expect(locked).toEqual(expect.objectContaining({ locked: "busy now", prunable: null }));
    expect(bare).toEqual(expect.objectContaining({ bare: true, head: null, branch: null }));
    expect(prunable).toEqual(
      expect.objectContaining({
        locked: "",
        prunable: "gitdir file points to non-existent location",
      }),
    );
  });

  it("keeps a path with a newline in it whole", () => {
    expect(parseWorktreeList(output(["worktree /a\nb", "HEAD aaa", "detached"]))).toEqual([
      expect.objectContaining({ path: "/a\nb", name: "a\nb" }),
    ]);
  });

  it("reads nothing from no output", () => {
    expect(parseWorktreeList("")).toEqual([]);
  });
});
