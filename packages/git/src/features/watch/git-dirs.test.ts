import { sep } from "node:path";

import { describe, expect, it } from "vitest";

import { classify, inside } from "./git-dirs";

const dirs = {
  gitDir: ["", "repo", ".git", "worktrees", "wt"].join(sep),
  commonDir: ["", "repo", ".git"].join(sep),
  excludeFile: ["", "repo", ".git", "info", "exclude"].join(sep),
};

/** A path in `dir`, with the platform's separators. */
const at = (dir: string, ...parts: string[]) => [dir, ...parts].join(sep);

describe("classify", () => {
  it("takes the index, HEAD and operations in progress for the worktree's own", () => {
    expect(classify(dirs, at(dirs.gitDir, "index"))).toBe("index");
    expect(classify(dirs, at(dirs.gitDir, "HEAD"))).toBe("refs");
    expect(classify(dirs, at(dirs.gitDir, "MERGE_HEAD"))).toBe("refs");
    expect(classify(dirs, at(dirs.gitDir, "rebase-merge", "done"))).toBe("refs");
    expect(classify(dirs, at(dirs.gitDir, "sequencer"))).toBe("refs");
  });

  it("takes the refs, packed refs, reftable and config for the shared ones", () => {
    expect(classify(dirs, at(dirs.commonDir, "refs", "heads", "main"))).toBe("refs");
    expect(classify(dirs, at(dirs.commonDir, "packed-refs"))).toBe("refs");
    expect(classify(dirs, at(dirs.commonDir, "reftable", "tables.list"))).toBe("refs");
    expect(classify(dirs, at(dirs.commonDir, "config"))).toBe("refs");
  });

  it("ignores lock files, objects, logs and files outside both directories", () => {
    expect(classify(dirs, at(dirs.gitDir, "index.lock"))).toBeUndefined();
    expect(classify(dirs, at(dirs.commonDir, "refs", "heads", "main.lock"))).toBeUndefined();
    expect(classify(dirs, at(dirs.commonDir, "objects", "ab", "cd"))).toBeUndefined();
    expect(classify(dirs, at(dirs.commonDir, "logs", "HEAD"))).toBeUndefined();
    expect(classify(dirs, ["", "repo", "src", "HEAD"].join(sep))).toBeUndefined();
  });

  it("takes the shared directory's HEAD for the main worktree's, when they're the same", () => {
    const main = { ...dirs, gitDir: dirs.commonDir };
    expect(classify(main, at(main.gitDir, "HEAD"))).toBe("refs");
    expect(classify(main, at(main.gitDir, "index"))).toBe("index");
  });
});

describe("inside", () => {
  it("is the path relative to the directory, with forward slashes", () => {
    expect(inside(dirs.commonDir, at(dirs.commonDir, "refs", "heads", "main"))).toBe(
      "refs/heads/main",
    );
  });

  it("is undefined for the directory itself, its parents and paths beside it", () => {
    expect(inside(dirs.commonDir, dirs.commonDir)).toBeUndefined();
    expect(inside(dirs.commonDir, ["", "repo"].join(sep))).toBeUndefined();
    expect(inside(dirs.commonDir, ["", "repo", ".github", "x"].join(sep))).toBeUndefined();
  });
});
