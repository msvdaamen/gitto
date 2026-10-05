import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { cloneRepo, createRepo, git, repos } from "../../test/fixtures";
import { getOverview } from "./commands";

describe("getOverview", () => {
  it("has nothing to tell of a new repository", async () => {
    createRepo("empty");
    expect(await getOverview(await repos.open("empty"))).toEqual({
      remote: null,
      fetchedAt: null,
      committedAt: null,
      activity: [],
    });
  });

  it("tells where a repository is hosted, when it was fetched and committed to, and what was done", async () => {
    const upstream = createRepo("upstream");
    git(upstream, "commit", "--allow-empty", "-qm", "first");
    const path = cloneRepo("clone", upstream);
    git(path, "checkout", "-qb", "feature");
    writeFileSync(join(path, "a.txt"), "a\n");
    git(path, "add", "a.txt");
    git(path, "commit", "-qm", "Add a");
    git(path, "fetch", "-q");

    const overview = await getOverview(await repos.open("clone"));
    expect(overview).toMatchObject({ remote: { name: "origin", url: upstream } });
    expect(overview.fetchedAt).toBeGreaterThan(Date.now() - 60_000);
    expect(overview.committedAt).toBe(Number(git(path, "log", "-1", "--format=%ct")) * 1000);
    expect(
      overview.activity.map(({ kind, target, subject }) => ({ kind, target, subject })),
    ).toEqual([
      { kind: "commit", target: null, subject: "Add a" },
      { kind: "checkout", target: "feature", subject: "first" },
      { kind: "clone", target: null, subject: "first" },
    ]);
  });
});
