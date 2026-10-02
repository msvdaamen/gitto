import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { git, paths, repos, root } from "../../test/fixtures";
import { fetchAll } from "./commands";

describe("fetchAll", () => {
  let upstream: string;
  let path: string;
  let repo: Repo;

  // `upstream` plays the remote: a repository the clone fetches from as `origin`.
  const commitUpstream = (message: string) =>
    git(upstream, "commit", "--allow-empty", "-qm", message);
  const remoteBranches = () =>
    git(path, "for-each-ref", "--format=%(refname:lstrip=2) %(subject)", "refs/remotes").split(
      "\n",
    );

  beforeAll(async () => {
    upstream = join(root, "upstream");
    git(root, "init", "-q", "-b", "main", upstream);
    git(upstream, "config", "user.name", "Test User");
    git(upstream, "config", "user.email", "test@example.com");
    commitUpstream("first");
    git(upstream, "branch", "feature");
    path = join(root, "clone");
    git(root, "clone", "-q", upstream, path);
    paths.set("clone", path);
    repo = await repos.open("clone");
  });

  it("fetches new commits and drops branches deleted on the remote", async () => {
    commitUpstream("second");
    git(upstream, "branch", "-D", "feature");
    await fetchAll(repo);
    expect(remoteBranches()).toEqual(["origin/HEAD second", "origin/main second"]);
  });

  it("does nothing without remotes", async () => {
    git(root, "init", "-q", "lonely");
    paths.set("lonely", join(root, "lonely"));
    await expect(fetchAll(await repos.open("lonely"))).resolves.toBeUndefined();
  });

  it("explains why a fetch failed", async () => {
    git(path, "remote", "set-url", "origin", join(root, "gone"));
    await expect(fetchAll(repo)).rejects.toMatchObject({
      message: expect.stringContaining("does not appear to be a git repository"),
    });
  });
});
