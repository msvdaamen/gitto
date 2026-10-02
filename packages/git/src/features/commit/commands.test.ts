import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { createRepo, git, page, repos } from "../../test/fixtures";
import { getLog } from "../history/commands";
import { createCommit, getCommitMessage } from "./commands";

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

  it("amends the last commit with what's staged and a new message", async () => {
    writeFileSync(join(path, "more.txt"), "more\n");
    git(path, "add", "more.txt");
    await createCommit(repo, "Amended commit", { amend: true });
    expect(await getLog(repo, page)).toEqual([
      expect.objectContaining({ subject: "Amended commit", body: "", parents: [] }),
    ]);
    expect(git(path, "show", "--name-only", "--format=", "HEAD").split("\n")).toEqual([
      "more.txt",
      "x y.txt",
    ]);
  });

  it("rewords the last commit with nothing staged", async () => {
    await createCommit(repo, "Reworded\n\nNew body", { amend: true });
    expect(await getLog(repo, page)).toEqual([
      expect.objectContaining({ subject: "Reworded", body: "New body", parents: [] }),
    ]);
  });

  it("explains why a commit failed", async () => {
    await expect(createCommit(repo, "nothing")).rejects.toMatchObject({
      message: expect.stringContaining("nothing to commit"),
    });
  });

  it("can't amend before the first commit", async () => {
    const unborn = createRepo("unborn");
    writeFileSync(join(unborn, "a.txt"), "a\n");
    git(unborn, "add", "a.txt");
    await expect(
      createCommit(await repos.open("unborn"), "Amend", { amend: true }),
    ).rejects.toMatchObject({ message: expect.stringContaining("nothing to amend") });
  });
});

describe("getCommitMessage", () => {
  it("is the message as written, first lines and all", async () => {
    const path = createRepo("message");
    const message = "First line\nsecond line\n\n#123 fixes it\n## Notes";
    git(path, "commit", "-q", "--allow-empty", "-m", message);
    expect(await getCommitMessage(await repos.open("message"), "HEAD")).toBe(message);
  });
});
