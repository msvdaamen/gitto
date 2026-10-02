import { beforeAll, describe, expect, it } from "vitest";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, page, rejection } from "../../test/fixtures";
import { getLog } from "../history/commands";
import { getCommitFiles } from "./commands";

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
