import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import { call } from "@orpc/server";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { CloneFailedError } from "../../core/errors";
import { GitVersion } from "../../core/version";
import {
  apiError,
  createClone,
  createRepo,
  git,
  rejection,
  repos,
  root,
} from "../../test/fixtures";
import { cloneRepository, clonePath, cloneSource, failure, parseProgress } from "./commands";
import { cloneRouter } from "./handler";
import type { CloneProgress } from "./schema";

/** Runs a clone to the end: what it reported along the way, and the path it returned. */
async function cloneAll(url: string, folder: string, signal?: AbortSignal) {
  const progress: CloneProgress[] = [];
  const clone = cloneRepository(url, folder, signal);
  while (true) {
    // oxlint-disable-next-line no-await-in-loop -- each step comes after the last.
    const step = await clone.next();
    if (step.done) return { progress, path: step.value };
    progress.push(step.value);
  }
}

/** Settings git takes from the environment, as `-c` would give them, for the clones run here. */
function stubConfig(settings: Record<string, string>) {
  const entries = Object.entries(settings);
  vi.stubEnv("GIT_CONFIG_COUNT", String(entries.length));
  entries.forEach(([key, value], index) => {
    vi.stubEnv(`GIT_CONFIG_KEY_${index}`, key);
    vi.stubEnv(`GIT_CONFIG_VALUE_${index}`, value);
  });
}

/** Resolves once `check` is true, checking every 20 ms. */
async function until(check: () => boolean) {
  // oxlint-disable-next-line no-await-in-loop -- polling.
  while (!check()) await new Promise((resolve) => setTimeout(resolve, 20));
}

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("cloneRepository", () => {
  let upstream: string;
  /** A repository whose `a.txt` is checked out through the filter `slow`, or `fail`. */
  let filtered: string;

  beforeAll(() => {
    ({ upstream } = createClone("clone-source"));
    filtered = createRepo("clone-filtered");
    writeFileSync(join(filtered, ".gitattributes"), "a.txt filter=checkout\n");
    writeFileSync(join(filtered, "a.txt"), "a\n");
    git(filtered, "add", "-A");
    git(filtered, "commit", "-qm", "filtered");
  });

  it("clones into a folder that isn't there yet, reporting its progress", async () => {
    const folder = join(root, "clones", "first");
    // A file URL, so git goes through a transport, which reports progress, rather than copying.
    const { progress, path } = await cloneAll(`file://${upstream}`, folder);

    expect(path).toBe(folder);
    expect(git(folder, "log", "--format=%s")).toBe("first");
    expect(git(folder, "rev-parse", "--abbrev-ref", "@{upstream}")).toBe("origin/main");
    expect(progress.length).toBeGreaterThan(0);
    for (const step of progress) {
      expect(step.phase).toMatch(/^[A-Z]/);
      expect(step.percent).toBeGreaterThanOrEqual(0);
      expect(step.percent).toBeLessThanOrEqual(100);
    }
  });

  it("clones into an empty folder", async () => {
    const folder = join(root, "clones", "empty");
    mkdirSync(folder, { recursive: true });
    await cloneAll(upstream, folder);
    expect(existsSync(join(folder, "a.txt"))).toBe(true);
  });

  it("refuses a folder with something in it, leaving it as it was", async () => {
    const folder = join(root, "clones", "full");
    mkdirSync(folder, { recursive: true });
    writeFileSync(join(folder, "keep.txt"), "mine");
    const error = await rejection(cloneAll(upstream, folder));
    expect(error).toBeInstanceOf(CloneFailedError);
    expect((error as Error).message).toBe(
      `${folder} isn't empty. Clone into an empty folder, or one that isn't there yet.`,
    );
    expect(existsSync(join(folder, "keep.txt"))).toBe(true);
    expect(existsSync(join(folder, ".git"))).toBe(false);
  });

  it("refuses a file where the folder would be, leaving it as it was", async () => {
    const file = join(root, "clones", "file.txt");
    mkdirSync(join(root, "clones"), { recursive: true });
    writeFileSync(file, "mine");
    const error = await rejection(cloneAll(upstream, file));
    expect((error as Error).message).toBe(
      `${file} is a file. Clone into an empty folder, or one that isn't there yet.`,
    );
    expect(existsSync(file)).toBe(true);
  });

  it("deletes a clone whose files couldn't be checked out", async () => {
    stubConfig({ "filter.checkout.smudge": "false", "filter.checkout.required": "true" });
    const folder = join(root, "clones", "unchecked");
    const error = await rejection(cloneAll(filtered, folder));
    expect(error).toBeInstanceOf(CloneFailedError);
    expect((error as Error).message).toMatch(
      /^The files couldn't be checked out, so the clone was deleted\.\n/,
    );
    expect((error as Error).message).not.toMatch(/git restore|Clone succeeded/);
    expect(existsSync(folder)).toBe(false);
  });

  it("leaves nothing of a clone stopped partway, and clones into its folder again", async () => {
    // Checking out a.txt takes a while, once the repository is there.
    stubConfig({ "filter.checkout.smudge": "sleep 2; cat" });
    const folder = join(root, "clones", "stopped");
    const controller = new AbortController();
    const stopped = rejection(cloneAll(filtered, folder, controller.signal));
    await until(() => existsSync(join(folder, ".git", "HEAD")));
    controller.abort();
    vi.unstubAllEnvs();
    // Started at once, before the first is cleaned up: it waits for that.
    const again = await cloneAll(upstream, folder);
    expect(((await stopped) as Error).name).toBe("AbortError");
    expect(again.path).toBe(folder);
    expect(git(folder, "log", "--format=%s")).toBe("first");
  });

  it("says why a URL couldn't be cloned, leaving nothing behind", async () => {
    const folder = join(root, "clones", "missing");
    const error = await rejection(cloneAll(join(root, "no-such-repository"), folder));
    expect(error).toBeInstanceOf(CloneFailedError);
    expect((error as Error).message).toMatch(/^Repository '.*no-such-repository' does not exist$/);
    expect(existsSync(folder)).toBe(false);
  });

  it("stops git when it's aborted", async () => {
    const folder = join(root, "clones", "aborted");
    const controller = new AbortController();
    controller.abort();
    const error = await rejection(cloneAll(`file://${upstream}`, folder, controller.signal));
    expect((error as Error).name).toBe("AbortError");
  });
});

describe("the clone procedure", () => {
  const context = { gitRepos: repos, gitVersion: new GitVersion() };

  it("reports a clone that failed as an error the renderer can show", async () => {
    const folder = join(root, "clones", "procedure");
    const run = async () => {
      const clone = await call(
        cloneRouter.run,
        { url: join(root, "no-such-repository"), path: folder },
        { context },
      );
      for await (const _ of clone);
    };
    expect(await apiError(run())).toMatchObject({ code: "PRECONDITION_FAILED" });
  });
});

describe("cloneSource", () => {
  it("takes a URL, a host:path, or a full path", () => {
    for (const url of [
      "https://github.com/owner/gitto.git",
      "ssh://git@example.com:2222/owner/gitto.git",
      "file:///srv/git/gitto",
      "git@github.com:owner/gitto.git",
      "example.com:gitto.git",
      "/srv/git/gitto",
    ]) {
      expect(cloneSource(` ${url} `)).toBe(url);
    }
    expect(cloneSource("~/code/gitto")).toBe(join(homedir(), "code", "gitto"));
  });

  it("refuses a relative path", () => {
    expect(() => cloneSource("code/gitto")).toThrow(CloneFailedError);
    expect(() => cloneSource("./gitto")).toThrow(CloneFailedError);
    expect(() => cloneSource("gitto")).toThrow(CloneFailedError);
  });
});

describe("clonePath", () => {
  it("takes a full path, or one from the home folder", () => {
    expect(clonePath("  /code/gitto/ ")).toBe("/code/gitto");
    expect(clonePath("~")).toBe(homedir());
    expect(clonePath("~/code/gitto")).toBe(join(homedir(), "code", "gitto"));
  });

  it("refuses a path that isn't a full one", () => {
    expect(() => clonePath("code/gitto")).toThrow(CloneFailedError);
    expect(() => clonePath("~someone/code")).toThrow(CloneFailedError);
  });
});

describe("parseProgress", () => {
  it("reads git's progress lines", () => {
    expect(parseProgress("Receiving objects:  45% (450/1000), 1.20 MiB | 600.00 KiB/s")).toEqual({
      phase: "Receiving objects",
      percent: 45,
    });
    expect(parseProgress("remote: Counting objects:  12% (1/8)")).toEqual({
      phase: "Counting objects",
      percent: 12,
    });
    expect(parseProgress("Resolving deltas: 100% (10/10), done.")).toEqual({
      phase: "Resolving deltas",
      percent: 100,
    });
  });

  it("skips other lines", () => {
    expect(parseProgress("Cloning into 'gitto'...")).toBeUndefined();
    expect(parseProgress("remote: Enumerating objects: 5, done.")).toBeUndefined();
    expect(parseProgress("fatal: repository 'x' not found")).toBeUndefined();
  });
});

describe("failure", () => {
  it("leaves out the progress and git's prefixes", () => {
    expect(
      failure(
        [
          "Cloning into 'gitto'...",
          "remote: Enumerating objects: 5, done.",
          "remote: Total 3 (delta 0), reused 0 (delta 0), pack-reused 0",
          "Receiving objects:  33% (1/3)\rReceiving objects: 100% (3/3), done.",
          "error: RPC failed; curl 92 HTTP/2 stream 0 was not closed cleanly",
          "fatal: early EOF",
          "",
        ].join("\n"),
      ),
    ).toBe("RPC failed; curl 92 HTTP/2 stream 0 was not closed cleanly\nEarly EOF");
  });

  it("puts a sentence git wraps over lines on one", () => {
    expect(
      failure(
        [
          "Cloning into 'gitto'...",
          "Host key verification failed.",
          "fatal: Could not read from remote repository.",
          "",
          "Please make sure you have the correct access rights",
          "and the repository exists.",
        ].join("\n"),
      ),
    ).toBe(
      "Host key verification failed.\nCould not read from remote repository.\nPlease make sure you have the correct access rights and the repository exists.",
    );
  });

  it("says a clone whose checkout failed was deleted, without git's advice", () => {
    expect(
      failure(
        [
          "Cloning into 'gitto'...",
          "error: external filter 'false' failed 1",
          "error: external filter 'false' failed",
          "fatal: a.txt: smudge filter checkout failed",
          "warning: Clone succeeded, but checkout failed.",
          "You can inspect what was checked out with 'git status'",
          "and retry with 'git restore --source=HEAD :/'",
          "",
        ].join("\n"),
      ),
    ).toBe(
      "The files couldn't be checked out, so the clone was deleted.\nExternal filter 'false' failed 1\nExternal filter 'false' failed\nA.txt: smudge filter checkout failed",
    );
  });

  it("says how to sign in when git couldn't ask for a password", () => {
    expect(
      failure(
        "Cloning into 'private'...\nfatal: could not read Username for 'https://github.com': terminal prompts disabled\n",
      ),
    ).toBe(
      "Git couldn't sign in to github.com. Set up a credential helper for it, or clone with an SSH URL.",
    );
  });
});
