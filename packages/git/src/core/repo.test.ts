import { rmSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { createRepo, rejection, repos } from "../test/fixtures";
import { FolderNotFoundError, NotARepositoryError, RepositoryNotFoundError } from "./errors";

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
