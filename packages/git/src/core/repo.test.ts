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

/** A repository whose index is out of date: its file was written again, unchanged. */
function createStaleRepo(name: string) {
  const path = createRepo(name);
  writeFileSync(join(path, "file.txt"), "x\n");
  git(path, "add", ".");
  git(path, "commit", "-q", "-m", "First");
  const later = new Date(Date.now() + 5000);
  utimesSync(join(path, "file.txt"), later, later);
  return { path, written: () => statSync(join(path, ".git", "index")).mtimeMs };
}

describe("refreshing the index", () => {
  it("is done after a slow first status, as a fresh clone's is", async () => {
    const { written } = createStaleRepo("refresh-slow");
    const repo = await repos.open("refresh-slow");
    const before = written();

    repo.statusTook(600);
    await vi.waitFor(() => expect(written()).toBeGreaterThan(before), { timeout: 3000 });
  });

  it("isn't after a quick first status, nor after a later one that's slow", async () => {
    const { written } = createStaleRepo("refresh-quick");
    const repo = await repos.open("refresh-quick");
    const before = written();

    repo.statusTook(20);
    repo.statusTook(600);
    await new Promise((resolve) => setTimeout(resolve, 1500));
    expect(written()).toBe(before);
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
