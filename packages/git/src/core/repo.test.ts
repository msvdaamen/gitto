import { rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it, vi } from "vitest";

import { createRepo, git, rejection, repos } from "../test/fixtures";
import { FolderNotFoundError, NotARepositoryError, RepositoryNotFoundError } from "./errors";
import { runGit } from "./runner";

describe("opening repositories", () => {
  it("rejects unknown repositories", async () => {
    await expect(repos.open("missing")).rejects.toBeInstanceOf(RepositoryNotFoundError);
  });

  it("rejects repositories whose folder was deleted", async () => {
    const path = createRepo("deleted");
    rmSync(path, { recursive: true });
    const error = await rejection(repos.open("deleted"));
    expect(error).toBeInstanceOf(FolderNotFoundError);
    expect(error).toMatchObject({ message: `${path} no longer exists.` });
  });
});

/**
 * A repository of a thousand files, whose index was written an hour before them, as far as git can
 * tell, or `after` them: before, it can't vouch for any of them, as in a fresh clone.
 */
function createClonedRepo(name: string, index: "before" | "after") {
  const path = createRepo(name);
  for (let i = 0; i < 1000; i++) writeFileSync(join(path, `file-${i}.txt`), "x\n");
  git(path, "add", ".");
  git(path, "commit", "-q", "-m", "First");
  const file = join(path, ".git", "index");
  const written = new Date(Date.now() + (index === "before" ? -3_600_000 : 10_000));
  utimesSync(file, written, written);
  return { path, written: () => statSync(file).mtimeMs };
}

describe("refreshing the index", () => {
  it("is done after a slow first status, when the index can't vouch for its files", async () => {
    const { written } = createClonedRepo("refresh-slow", "before");
    const repo = await repos.open("refresh-slow");
    const before = written();

    repo.statusTook(600);
    await vi.waitFor(() => expect(written()).toBeGreaterThan(before), { timeout: 3000 });
  });

  it("isn't after a first status that was slow for another reason", async () => {
    const { written } = createClonedRepo("refresh-sure", "after");
    const repo = await repos.open("refresh-sure");
    const before = written();

    repo.statusTook(600);
    await new Promise((resolve) => setTimeout(resolve, 1500));
    expect(written()).toBe(before);
  });

  it("isn't after a quick first status, nor after a later one that's slow", async () => {
    const { written } = createClonedRepo("refresh-quick", "before");
    const repo = await repos.open("refresh-quick");
    const before = written();

    repo.statusTook(20);
    repo.statusTook(600);
    await new Promise((resolve) => setTimeout(resolve, 1500));
    expect(written()).toBe(before);
  });

  it("is done after a command that rewrites files, also one that fails", async () => {
    const { written } = createClonedRepo("refresh-failed", "before");
    const repo = await repos.open("refresh-failed");
    const before = written();

    await expect(repo.write(["switch", "missing"], { rewritesFiles: true })).rejects.toThrow();
    await vi.waitFor(() => expect(written()).toBeGreaterThan(before), { timeout: 3000 });
  });
});

describe("running commands", () => {
  it("recognises a folder that's no longer a repository", async () => {
    const path = createRepo("unrepo");
    const repo = await repos.open("unrepo");
    rmSync(join(path, ".git"), { recursive: true });
    const error = await rejection(repo.read(["status"]));
    expect(error).toBeInstanceOf(NotARepositoryError);
    expect(error).toMatchObject({ message: `${path} is no longer a git repository.` });
  });

  it("rejects with the abort instead of a git error when cancelled", async () => {
    createRepo("cancelled");
    const repo = await repos.open("cancelled");
    await expect(repo.read(["log"], { signal: AbortSignal.abort() })).rejects.toMatchObject({
      name: "AbortError",
    });
  });
});

describe("exclusive writes", () => {
  it("keep other writes out until they're done", async () => {
    createRepo("exclusive");
    const repo = await repos.open("exclusive");
    const order: string[] = [];
    let finish!: () => void;
    const exclusive = repo.exclusive(async (run) => {
      await run(["status"]);
      await new Promise<void>((resolve) => (finish = resolve));
      order.push("exclusive");
    });
    const write = repo.write(["status"]).then(() => order.push("write"));
    await vi.waitFor(() => expect(finish).toBeDefined());
    finish();
    await Promise.all([exclusive, write]);
    expect(order).toEqual(["exclusive", "write"]);
  });
});

describe("settings for one command", () => {
  it("apply to it, and stay out of its arguments", async () => {
    const path = createRepo("settings");
    expect(
      await runGit(path, ["config", "--get", "gitto.test"], { config: ["gitto.test=1"] }),
    ).toBe("1\n");
    const error = await rejection(
      runGit(path, ["rev-parse", "--verify", "nope"], { config: ["gc.auto=0"] }),
    );
    expect(error).toMatchObject({ args: ["rev-parse", "--verify", "nope"] });
  });
});
