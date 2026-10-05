import { execFileSync } from "node:child_process";

import { beforeAll, describe, expect, it } from "vitest";

import { GitError } from "../../core/errors";
import { FileTooLargeError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, page, paths, rejection } from "../../test/fixtures";
import { getLog } from "../history/commands";
import { getBlob, getCommitFilePatch, getCommitFiles, MAX_BLOB_BYTES } from "./commands";

describe("getCommitFiles", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo();
  });

  it("diffs a merge against its first parent, and a root commit against nothing", async () => {
    const log = await getLog(repo, page);
    const merge = log.find((commit) => commit.subject === "Merge side");
    const first = log.find((commit) => commit.subject === "first");
    expect(await getCommitFiles(repo, merge!.sha)).toEqual([
      { path: "c.txt", status: "renamed", origPath: "b.txt", additions: 0, deletions: 0 },
      { path: "s.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
    ]);
    expect(await getCommitFiles(repo, first!.sha)).toEqual([
      { path: "a file.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "b.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "bin.dat", status: "added", origPath: null, additions: null, deletions: null },
    ]);
  });

  it("explains why a command failed", async () => {
    const error = await rejection(getCommitFiles(repo, "0123456789abcdef0123456789abcdef01234567"));
    expect(error).toBeInstanceOf(GitError);
    expect(error).toMatchObject({ message: expect.stringContaining("bad object") });
  });
});

describe("getCommitFilePatch", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo("patches");
  });

  async function shaOf(subject: string) {
    const log = await getLog(repo, page);
    return log.find((commit) => commit.subject === subject)!.sha;
  }

  it("shows one file's changes, with its full object names", async () => {
    const patch = await getCommitFilePatch(repo, await shaOf("main commit"), {
      path: "a file.txt",
      origPath: null,
    });
    expect(patch).toMatch(
      /^diff --git a\/a file.txt b\/a file.txt\nindex [0-9a-f]{40}\.\.[0-9a-f]{40} 100644\n/,
    );
    expect(patch).toContain("@@ -1 +1,2 @@\n a\n+more\n");
  });

  it("finds a rename with its old path, compared to a merge's first parent", async () => {
    const patch = await getCommitFilePatch(repo, await shaOf("Merge side"), {
      path: "c.txt",
      origPath: "b.txt",
    });
    expect(patch).toContain("rename from b.txt\nrename to c.txt\n");
  });

  it("shows a root commit's files as added", async () => {
    const patch = await getCommitFilePatch(repo, await shaOf("first"), {
      path: "b.txt",
      origPath: null,
    });
    expect(patch).toContain("new file mode 100644\n");
    expect(patch).toContain("@@ -0,0 +1 @@\n+b\n");
  });
});

describe("getBlob", () => {
  it("reads a file by the object name on a patch's index line", async () => {
    const repo = await createHistoryRepo("blobs");
    const log = await getLog(repo, page);
    const sha = log.find((commit) => commit.subject === "main commit")!.sha;
    const patch = await getCommitFilePatch(repo, sha, { path: "a file.txt", origPath: null });
    const [, before, after] = /^index ([0-9a-f]+)\.\.([0-9a-f]+)/m.exec(patch)!;
    expect(await getBlob(repo, before!)).toBe("a\n");
    expect(await getBlob(repo, after!)).toBe("a\nmore\n");
  });

  it("won't read a file too large to send whole", async () => {
    const repo = await createHistoryRepo("large-blob");
    const oid = execFileSync("git", ["hash-object", "-w", "--stdin"], {
      cwd: paths.get("large-blob"),
      input: "x".repeat(MAX_BLOB_BYTES + 1),
      encoding: "utf8",
    }).trim();
    expect(await rejection(getBlob(repo, oid))).toBeInstanceOf(FileTooLargeError);
  });
});
