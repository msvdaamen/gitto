import { beforeAll, describe, expect, it } from "vitest";

import { GitError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, page, rejection, repos } from "../../test/fixtures";
import { getCommit, getLog } from "./commands";

describe("a repository with history", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo();
  });

  it("reads the log of branches and tags, but not other refs", async () => {
    const { commits: log } = await getLog(repo, page);
    // Both sides of the merge have the same timestamp, so their order is up to git.
    expect(log.map((commit) => commit.subject).toSorted()).toEqual([
      "Merge side",
      "first",
      "main commit",
      "side commit",
    ]);
    expect(log[0]).toMatchObject({
      refs: [
        { kind: "local", name: "main", current: true },
        { kind: "tag", name: "v1" },
      ],
      authorName: "Test User",
    });
    expect(log[0]!.parents).toHaveLength(2);
  });

  it("reads the log a page at a time, with a version that follows the refs", async () => {
    const all = await getLog(repo, page);
    const first = await getLog(repo, { limit: 3, skip: 0 });
    const rest = await getLog(repo, { limit: 3, skip: 3 });
    expect([...first.commits, ...rest.commits]).toEqual(all.commits);
    // Fewer than asked for: that's where the history ends.
    expect(rest.commits).toHaveLength(1);

    expect((await getLog(repo, page)).version).toBe(all.version);
    git(repo.path, "tag", "v2");
    expect((await getLog(repo, page)).version).not.toBe(all.version);
    git(repo.path, "tag", "-d", "v2");
  });

  it("reads a single commit", async () => {
    const [head] = (await getLog(repo, page)).commits;
    expect(await getCommit(repo, head!.sha.slice(0, 7))).toEqual(head);

    const tree = git(repo.path, "rev-parse", "HEAD^{tree}");
    const error = await rejection(getCommit(repo, tree));
    expect(error).toBeInstanceOf(GitError);
    expect(error).toMatchObject({ message: `${tree} is not a commit.` });
  });
});

describe("a repository without commits", () => {
  it("has an empty log", async () => {
    createRepo("empty");
    expect((await getLog(await repos.open("empty"), page)).commits).toEqual([]);
  });
});
