import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import { HeadMovedError, NotARepositoryError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createRepo, git, page, paths, repos, root } from "../../test/fixtures";
import { getLog } from "../history/commands";
import { createCommit, getCommitMessage, getPushedTo } from "./commands";

describe("createCommit", () => {
  let path: string;
  let repo: Repo;

  beforeAll(async () => {
    path = createRepo("empty");
    repo = await repos.open("empty");
  });

  const head = () => git(path, "rev-parse", "HEAD");

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
    await createCommit(repo, "Amended commit", { amend: head() });
    expect(await getLog(repo, page)).toEqual([
      expect.objectContaining({ subject: "Amended commit", body: "", parents: [] }),
    ]);
    expect(git(path, "show", "--name-only", "--format=", "HEAD").split("\n")).toEqual([
      "more.txt",
      "x y.txt",
    ]);
  });

  it("rewords the last commit with nothing staged", async () => {
    await createCommit(repo, "Reworded\n\nNew body", { amend: head() });
    expect(await getLog(repo, page)).toEqual([
      expect.objectContaining({ subject: "Reworded", body: "New body", parents: [] }),
    ]);
  });

  it("rewords an empty commit", async () => {
    git(path, "commit", "-q", "--allow-empty", "-m", "Empty");
    await createCommit(repo, "Still empty", { amend: head() });
    expect((await getLog(repo, page))[0]).toMatchObject({ subject: "Still empty" });
  });

  it("amends before a commit queued while it checks HEAD, not that commit", async () => {
    const amending = createCommit(repo, "Amended first", { amend: head() });
    const other = repo.write(["commit", "-q", "--allow-empty", "-m", "Queued after"]);
    await Promise.all([amending, other]);
    expect((await getLog(repo, page)).map((commit) => commit.subject).slice(0, 2)).toEqual([
      "Queued after",
      "Amended first",
    ]);
  });

  it("amends with the message exactly as written, whatever commit.cleanup says", async () => {
    git(path, "config", "commit.cleanup", "strip");
    const message = "Kept  \n\n\n#123 fixes it\nHard break  \n";
    await createCommit(repo, message, { amend: head() });
    git(path, "config", "--unset", "commit.cleanup");
    expect(await getCommitMessage(repo, "HEAD")).toBe(message);
  });

  it("records the message in UTF-8 whatever i18n.commitEncoding says", async () => {
    git(path, "config", "i18n.commitEncoding", "ISO-8859-1");
    await createCommit(repo, "Café", { amend: head() });
    git(path, "config", "--unset", "i18n.commitEncoding");
    expect(await getCommitMessage(repo, "HEAD")).toBe("Café\n");
  });

  it("explains why a commit failed", async () => {
    await expect(createCommit(repo, "nothing")).rejects.toMatchObject({
      message: expect.stringContaining("nothing to commit"),
    });
  });

  it("won't amend once HEAD has moved on", async () => {
    const amending = head();
    git(path, "commit", "-q", "--allow-empty", "-m", "Made in a terminal");
    await expect(createCommit(repo, "Amend", { amend: amending })).rejects.toBeInstanceOf(
      HeadMovedError,
    );
    expect((await getLog(repo, page))[0]).toMatchObject({ subject: "Made in a terminal" });
  });

  it("reports why it couldn't check HEAD", async () => {
    const gone = createRepo("gone");
    const goneRepo = await repos.open("gone");
    rmSync(join(gone, ".git"), { recursive: true });
    await expect(createCommit(goneRepo, "Amend", { amend: "a".repeat(40) })).rejects.toBeInstanceOf(
      NotARepositoryError,
    );
  });

  it("can't amend before the first commit", async () => {
    const unborn = createRepo("unborn");
    writeFileSync(join(unborn, "a.txt"), "a\n");
    git(unborn, "add", "a.txt");
    await expect(
      createCommit(await repos.open("unborn"), "Amend", { amend: "a".repeat(40) }),
    ).rejects.toBeInstanceOf(HeadMovedError);
  });
});

describe("getCommitMessage", () => {
  it("is the message as written, first lines and all, in UTF-8", async () => {
    const path = createRepo("message");
    const message = "First line\nsecond line\n\n#123 fixes it\n## Café";
    git(path, "commit", "-q", "--allow-empty", "-m", message);
    git(path, "config", "i18n.logOutputEncoding", "ISO-8859-1");
    expect(await getCommitMessage(await repos.open("message"), "HEAD")).toBe(`${message}\n`);
  });
});

describe("getPushedTo", () => {
  it("is the remote branch git push would update, if it has the commit", async () => {
    const remote = createRepo("remote");
    git(remote, "commit", "-q", "--allow-empty", "-m", "First");
    const path = join(root, "clone");
    git(root, "clone", "-q", remote, path);
    paths.set("clone", path);
    const repo = await repos.open("clone");
    expect(await getPushedTo(repo, "HEAD")).toBe("origin/main");

    // Only created from origin/main, which tracks it: amending rewrites `feature` alone.
    git(path, "switch", "-q", "-c", "feature", "origin/main");
    expect(await getPushedTo(repo, "HEAD")).toBeNull();

    git(path, "commit", "-q", "--allow-empty", "-m", "Feature");
    expect(await getPushedTo(repo, "HEAD")).toBeNull();
    // Pushed without setting it as the upstream.
    git(path, "push", "-q", "origin", "feature");
    expect(await getPushedTo(repo, "HEAD")).toBe("origin/feature");

    // Pushed under another name: found through the push config.
    git(path, "switch", "-q", "-c", "mine");
    git(path, "push", "-q", "-u", "origin", "mine:theirs");
    expect(await getPushedTo(repo, "HEAD")).toBeNull();
    git(path, "config", "push.default", "upstream");
    expect(await getPushedTo(repo, "HEAD")).toBe("origin/theirs");

    // A tag of the same name doesn't confuse which branch is checked out.
    git(path, "config", "--unset", "push.default");
    git(path, "switch", "-q", "feature");
    git(path, "tag", "feature");
    expect(await getPushedTo(repo, "HEAD")).toBe("origin/feature");

    // Pushed to a branch of this repository.
    git(path, "config", "push.default", "upstream");
    git(path, "branch", "-q", "--set-upstream-to=main");
    git(path, "config", "branch.feature.remote", ".");
    expect(await getPushedTo(repo, "HEAD")).toBeNull();

    git(path, "switch", "-q", "--detach");
    expect(await getPushedTo(repo, "HEAD")).toBeNull();
  });
});
