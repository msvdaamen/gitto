import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { call } from "@orpc/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { InitFailedError } from "../../core/errors";
import { GitVersion } from "../../core/version";
import { apiError, git, rejection, repos, root } from "../../test/fixtures";
import { failure, initRepository } from "./commands";
import { initRouter } from "./handler";

/** A global git config that sets `init.defaultBranch` to `branch`, used while the test runs. */
function defaultBranch(branch: string | undefined) {
  const config = join(root, `global-${branch ?? "unset"}.gitconfig`);
  writeFileSync(config, branch ? `[init]\n\tdefaultBranch = ${branch}\n` : "");
  vi.stubEnv("GIT_CONFIG_GLOBAL", config);
  vi.stubEnv("GIT_CONFIG_NOSYSTEM", "1");
}

describe("initRepository", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("creates the folder and an empty repository on main in it", async () => {
    defaultBranch(undefined);
    const folder = join(root, "inits", "new", "nested");
    expect(await initRepository(folder)).toBe(folder);
    expect(git(folder, "rev-parse", "--show-toplevel")).toBe(folder);
    expect(git(folder, "symbolic-ref", "HEAD")).toBe("refs/heads/main");
  });

  it("starts on the branch the user's init.defaultBranch names", async () => {
    defaultBranch("trunk");
    const folder = join(root, "inits", "trunk");
    await initRepository(folder);
    expect(git(folder, "symbolic-ref", "HEAD")).toBe("refs/heads/trunk");
  });

  it("goes by the user's config, not that of a repository the home folder is in", async () => {
    defaultBranch(undefined);
    const home = join(root, "inits", "home");
    git(root, "init", "-q", "-b", "main", home);
    git(home, "config", "init.defaultBranch", "local");
    vi.stubEnv("HOME", home);
    const folder = join(home, "project");
    await initRepository(folder);
    expect(git(folder, "symbolic-ref", "HEAD")).toBe("refs/heads/main");
  });

  it("keeps what's in a folder that's there already", async () => {
    const folder = join(root, "inits", "existing");
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, "keep.txt"), "mine");
    await initRepository(`${folder}/`);
    expect(readFileSync(join(folder, "keep.txt"), "utf8")).toBe("mine");
    expect(git(folder, "status", "--porcelain")).toBe("?? keep.txt");
  });

  it("refuses a folder that's a repository already, leaving it as it was", async () => {
    const folder = join(root, "inits", "twice");
    await initRepository(folder);
    writeFileSync(join(folder, ".git", "description"), "mine");
    const error = await rejection(initRepository(folder));
    expect(error).toBeInstanceOf(InitFailedError);
    expect((error as Error).message).toBe(
      `${folder} is a git repository already. Open it instead.`,
    );
    expect(readFileSync(join(folder, ".git", "description"), "utf8")).toBe("mine");
  });

  it("refuses a path that isn't a full one", async () => {
    const error = await rejection(initRepository("code/gitto"));
    expect(error).toBeInstanceOf(InitFailedError);
    expect((error as Error).message).toBe(
      "code/gitto isn't a full path. Choose the folder to create it in, or type its full path.",
    );
  });

  it("refuses a file", async () => {
    const file = join(root, "inits", "taken");
    writeFileSync(file, "mine");
    const error = await rejection(initRepository(file));
    expect(error).toBeInstanceOf(InitFailedError);
    expect((error as Error).message).toBe(
      `${file} is a file. Choose a folder, or a name that isn't taken.`,
    );
    expect(readFileSync(file, "utf8")).toBe("mine");
  });

  it("says why git couldn't create it", async () => {
    const file = join(root, "inits", "a-file");
    writeFileSync(file, "");
    const error = await rejection(initRepository(join(file, "repo")));
    expect(error).toBeInstanceOf(InitFailedError);
    // Without git's `fatal:`, e.g. `Cannot mkdir …/a-file/repo: File exists`.
    expect((error as Error).message).toMatch(/^Cannot mkdir .*a-file\/repo: /);
  });
});

describe("the init procedure", () => {
  const context = { gitRepos: repos, gitVersion: new GitVersion() };

  it("reports a repository that couldn't be created as an error the renderer can show", async () => {
    const folder = join(root, "inits", "procedure");
    await initRepository(folder);
    const error = await apiError(call(initRouter.run, { path: folder }, { context }));
    expect(error).toMatchObject({ code: "PRECONDITION_FAILED" });
  });
});

describe("failure", () => {
  it("leaves out git's hints and prefixes", () => {
    expect(
      failure(
        [
          "hint: Using 'master' as the name for the initial branch.",
          "fatal: cannot mkdir /srv/gitto: Permission denied",
          "",
        ].join("\n"),
      ),
    ).toBe("Cannot mkdir /srv/gitto: Permission denied");
  });

  it("says something when git didn't", () => {
    expect(failure("")).toBe("Git couldn't create the repository.");
  });
});
