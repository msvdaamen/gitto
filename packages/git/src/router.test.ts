import { writeFileSync } from "node:fs";
import { join } from "node:path";

import { call } from "@orpc/server";
import { beforeAll, describe, expect, it } from "vitest";

import { GitVersion } from "./core/version";
import { gitRouter } from "./router";
import { apiError, createHistoryRepo, createRepo, paths, repos } from "./test/fixtures";

describe("the router", () => {
  // Procedures only take UUIDv7 repository ids.
  const ids = {
    history: "01920000-0000-7000-8000-000000000001",
    locked: "01920000-0000-7000-8000-000000000002",
    missing: "01920000-0000-7000-8000-000000000003",
  };
  const context = { gitRepos: repos, gitVersion: new GitVersion() };

  beforeAll(async () => {
    await createHistoryRepo();
    const locked = createRepo("locked");
    writeFileSync(join(locked, "file.txt"), "x");
    writeFileSync(join(locked, ".git", "index.lock"), "");
    paths.set(ids.history, paths.get("history")!);
    paths.set(ids.locked, locked);
  });

  it("reports git errors as API errors the renderer can show", async () => {
    expect(
      await apiError(call(gitRouter.status.get, { repositoryId: ids.missing }, { context })),
    ).toMatchObject({ code: "NOT_FOUND", message: "Repository not found." });

    expect(
      await apiError(
        call(
          gitRouter.staging.stage,
          { repositoryId: ids.locked, paths: ["file.txt"] },
          { context },
        ),
      ),
    ).toMatchObject({
      code: "CONFLICT",
      message: "Another git process is running in this repository.",
    });

    expect(
      await apiError(
        call(
          gitRouter.diff.commitFiles,
          { repositoryId: ids.history, sha: "0123456789abcdef0123456789abcdef01234567" },
          { context },
        ),
      ),
    ).toMatchObject({
      code: "INTERNAL_SERVER_ERROR",
      message: expect.stringContaining("bad object"),
    });

    expect(
      await apiError(call(gitRouter.remote.pull, { repositoryId: ids.history }, { context })),
    ).toMatchObject({
      code: "PRECONDITION_FAILED",
      message: "main doesn't track a remote branch.",
    });

    expect(
      await apiError(
        call(
          gitRouter.stash.pop,
          { repositoryId: ids.history, sha: "0123456789abcdef0123456789abcdef01234567" },
          { context },
        ),
      ),
    ).toMatchObject({
      code: "CONFLICT",
      message: "The stash is gone: the stashes changed before it could be popped.",
    });
  });

  it("runs nothing with a git Gitto doesn't support", async () => {
    let output: string | null = "git version 2.39.3 (Apple Git-146)\n";
    const gitVersion = new GitVersion(async () => output);
    const outdated = { gitRepos: repos, gitVersion };

    expect(await call(gitRouter.version.check, undefined, { context: outdated })).toEqual({
      version: "2.39.3",
      required: "2.41",
      supported: false,
    });
    expect(
      await apiError(
        call(gitRouter.status.get, { repositoryId: ids.history }, { context: outdated }),
      ),
    ).toMatchObject({
      code: "PRECONDITION_FAILED",
      message: "Gitto needs Git 2.41 or newer, but Git 2.39.3 is installed.",
    });

    output = null;
    expect(
      await apiError(
        call(gitRouter.status.get, { repositoryId: ids.history }, { context: outdated }),
      ),
    ).toMatchObject({
      code: "PRECONDITION_FAILED",
      message: "Gitto couldn't find Git. Install Git 2.41 or newer.",
    });

    // Updated since: it works again without a restart.
    output = "git version 2.51.0\n";
    expect(
      await call(gitRouter.status.get, { repositoryId: ids.history }, { context: outdated }),
    ).toMatchObject({ head: { kind: "branch", name: "main" } });
  });

  it("skips a status the caller already has", async () => {
    const input = { repositoryId: ids.history };
    const status = await call(gitRouter.status.get, input, { context });
    if ("unchanged" in status) return expect.fail("expected a full status");

    expect(
      await call(gitRouter.status.get, { ...input, since: status.version }, { context }),
    ).toEqual({ unchanged: true });
    expect(
      await call(gitRouter.status.get, { ...input, since: "an older version" }, { context }),
    ).toEqual(status);
  });
});
