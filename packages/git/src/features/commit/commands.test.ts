import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { createRepo, git, page, repos } from "../../test/fixtures";
import { getLog } from "../history/commands";
import { createCommit } from "./commands";

describe("createCommit", () => {
  let path: string;
  let repo: Repo;

  beforeAll(async () => {
    path = createRepo("empty");
    repo = await repos.open("empty");
  });

  it("commits what's staged, with the message's body", async () => {
    writeFileSync(join(path, "x y.txt"), "hi\n");
    git(path, "add", "x y.txt");
    await createCommit(repo, "Initial commit\n\nWith body");
    expect(await getLog(repo, page)).toEqual([
      expect.objectContaining({
        subject: "Initial commit",
        body: "With body",
        refs: [{ kind: "local", name: "main", current: true }],
      }),
    ]);
  });

  it("explains why a commit failed", async () => {
    await expect(createCommit(repo, "nothing")).rejects.toMatchObject({
      message: expect.stringContaining("nothing to commit"),
    });
  });
});
