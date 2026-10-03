import { rmSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it, vi } from "vitest";

import { createRepo, rejection, repos } from "../test/fixtures";
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
