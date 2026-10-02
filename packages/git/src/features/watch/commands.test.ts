import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { createRepo, git, paths, repos, root } from "../../test/fixtures";
import { stage } from "../staging/commands";
import { watchGitDir, watchWorkingTree } from "./commands";

/** Whether `next` stays pending for longer than the watcher's debounce: resolves to "quiet" if so. */
function quiet(next: Promise<unknown>): Promise<unknown> {
  return Promise.race([next, new Promise((resolve) => setTimeout(() => resolve("quiet"), 700))]);
}

/**
 * Waits for what was just written to the git directory (like `createRepo`'s config) to settle:
 * under load, the OS can report it to a watch started right after.
 */
function settled(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 300));
}

/** Runs `change` once the watcher has had time to start; it only sees what happens after that. */
function soon(change: () => void) {
  setTimeout(change, 200);
}

describe("watching the working tree", () => {
  it("emits after a change and stops when aborted", async () => {
    const path = createRepo("watched");
    const repo = await repos.open("watched");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    expect(await changes.next()).toEqual({ value: "ready", done: false });
    const next = changes.next();
    writeFileSync(join(path, "file.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    controller.abort();
    expect(await changes.next()).toEqual({ value: undefined, done: true });
  });

  it("leaves the git directory out", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-git-dir");
    const repo = await repos.open("watched-git-dir");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    expect(await changes.next()).toEqual({ value: "ready", done: false });
    let next = changes.next();
    writeFileSync(join(path, "file.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    next = changes.next();
    git(path, "add", "file.txt");
    expect(await quiet(next)).toBe("quiet");

    controller.abort();
  });

  it("skips ignored directories and follows new ones", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-tree");
    writeFileSync(join(path, ".gitignore"), "build/\n");
    mkdirSync(join(path, "build"));
    const repo = await repos.open("watched-tree");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    expect(await changes.next()).toEqual({ value: "ready", done: false });
    let next = changes.next();
    writeFileSync(join(path, "file.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    // Ignored.
    next = changes.next();
    writeFileSync(join(path, "build", "out.txt"), "x");
    expect(await quiet(next)).toBe("quiet");

    // Created (and reported) afterwards; changes in it are reported too.
    mkdirSync(join(path, "src"));
    expect(await next).toEqual({ value: "changed", done: false });
    next = changes.next();
    writeFileSync(join(path, "src", "a.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    controller.abort();
  });

  it("skips ignored folders created after it started", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-new-ignored");
    writeFileSync(join(path, ".gitignore"), "dist/\n*.log\n");
    const repo = await repos.open("watched-new-ignored");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    expect(await changes.next()).toEqual({ value: "ready", done: false });
    let next = changes.next();
    writeFileSync(join(path, "file.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    // Like a build: a new ignored folder, written to repeatedly.
    next = changes.next();
    mkdirSync(join(path, "dist"));
    writeFileSync(join(path, "dist", "a.js"), "x");
    expect(await quiet(next)).toBe("quiet");
    writeFileSync(join(path, "dist", "b.js"), "x");
    writeFileSync(join(path, "debug.log"), "x");
    expect(await quiet(next)).toBe("quiet");

    // A change that shows, alongside ignored ones, is still reported.
    writeFileSync(join(path, "dist", "c.js"), "x");
    writeFileSync(join(path, "file.txt"), "y");
    expect(await next).toEqual({ value: "changed", done: false });

    controller.abort();
  });

  it("reports many changes at once, ignored ones among them", { timeout: 20_000 }, async () => {
    const path = createRepo("watched-many");
    writeFileSync(join(path, ".gitignore"), "*.log\n");
    const repo = await repos.open("watched-many");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    expect(await changes.next()).toEqual({ value: "ready", done: false });
    let next = changes.next();
    writeFileSync(join(path, "file.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    // More than git is asked about at once, and only ignored files: nothing to show.
    next = changes.next();
    for (let i = 0; i < 300; i++) writeFileSync(join(path, `${i}.log`), "x");
    expect(await quiet(next)).toBe("quiet");

    // One file that shows, after hundreds that don't.
    for (let i = 0; i < 300; i++) writeFileSync(join(path, `${i}.log`), "y");
    writeFileSync(join(path, "zzz.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    controller.abort();
  });

  it("follows changes to the ignore rules", { timeout: 10_000 }, async () => {
    const path = createRepo("watched-rules");
    writeFileSync(join(path, ".gitignore"), "build/\n");
    mkdirSync(join(path, "build"));
    mkdirSync(join(path, "src"));
    const repo = await repos.open("watched-rules");
    const controller = new AbortController();
    const changes = watchWorkingTree(repo, controller.signal);

    expect(await changes.next()).toEqual({ value: "ready", done: false });
    let next = changes.next();
    writeFileSync(join(path, "file.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    // `build` is no longer ignored, `src` now is.
    next = changes.next();
    writeFileSync(join(path, ".gitignore"), "src/\n");
    expect(await next).toEqual({ value: "changed", done: false });

    next = changes.next();
    writeFileSync(join(path, "build", "out.txt"), "x");
    expect(await next).toEqual({ value: "changed", done: false });

    next = changes.next();
    writeFileSync(join(path, "src", "a.txt"), "x");
    expect(await quiet(next)).toBe("quiet");

    // Rules in .git/info/exclude count too.
    writeFileSync(join(path, ".git", "info", "exclude"), "build/\n");
    expect(await next).toEqual({ value: "changed", done: false });
    next = changes.next();
    writeFileSync(join(path, "build", "out.txt"), "y");
    expect(await quiet(next)).toBe("quiet");

    controller.abort();
  });
});

describe("watching the git directory", () => {
  it(
    "reports staging, commits and checkouts, but not the working tree",
    { timeout: 10_000 },
    async () => {
      const path = createRepo("watched-refs");
      const repo = await repos.open("watched-refs");
      await settled();
      const controller = new AbortController();
      const changes = watchGitDir(repo, controller.signal);

      let next = changes.next();
      soon(() => {
        writeFileSync(join(path, "file.txt"), "x");
        git(path, "add", "file.txt");
      });
      expect(await next).toEqual({ value: ["index"], done: false });

      next = changes.next();
      git(path, "commit", "-q", "-m", "First");
      expect((await next).value).toContain("refs");

      next = changes.next();
      git(path, "checkout", "-q", "-b", "feature");
      expect((await next).value).toContain("refs");

      next = changes.next();
      writeFileSync(join(path, "file.txt"), "y");
      expect(await quiet(next)).toBe("quiet");

      controller.abort();
      expect(await changes.next()).toEqual({ value: undefined, done: true });
    },
  );

  it(
    "leaves out Gitto's own staging, which the UI refetches after itself",
    { timeout: 10_000 },
    async () => {
      const path = createRepo("watched-own");
      writeFileSync(join(path, "file.txt"), "x");
      writeFileSync(join(path, "other.txt"), "x");
      const repo = await repos.open("watched-own");
      await settled();
      const controller = new AbortController();
      const changes = watchGitDir(repo, controller.signal);

      const next = changes.next();
      await new Promise((resolve) => setTimeout(resolve, 200));
      // Through another handle on the repository, like every call from the UI.
      await stage(await repos.open("watched-own"), ["file.txt"]);
      expect(await quiet(next)).toBe("quiet");

      // Another tool's staging is still reported.
      git(path, "add", "other.txt");
      expect(await next).toEqual({ value: ["index"], done: false });

      controller.abort();
    },
  );

  it(
    "keeps going while the working tree is watched and unwatched",
    { timeout: 10_000 },
    async () => {
      const path = createRepo("watched-both");
      const repo = await repos.open("watched-both");
      await settled();
      const controller = new AbortController();
      const changes = watchGitDir(repo, controller.signal);
      let next = changes.next();

      // Like the UI does when the window gets and loses focus.
      const tree = new AbortController();
      const treeChanges = watchWorkingTree(repo, tree.signal);
      expect(await treeChanges.next()).toEqual({ value: "ready", done: false });
      const treeNext = treeChanges.next();
      writeFileSync(join(path, "file.txt"), "x");
      expect(await treeNext).toEqual({ value: "changed", done: false });
      tree.abort();
      expect(await treeChanges.next()).toEqual({ value: undefined, done: true });

      git(path, "add", "file.txt");
      expect(await next).toEqual({ value: ["index"], done: false });
      next = changes.next();
      writeFileSync(join(path, ".git", "info", "exclude"), "build/\n");
      git(path, "commit", "-q", "-m", "First");
      expect((await next).value).toContain("refs");

      controller.abort();
    },
  );

  it("reports refs stored in a reftable", { timeout: 10_000 }, async () => {
    // What `git init --ref-format=reftable` (git 2.45+) writes to, instead of `refs`.
    const path = createRepo("watched-reftable");
    mkdirSync(join(path, ".git", "reftable"));
    const repo = await repos.open("watched-reftable");
    await settled();
    const controller = new AbortController();
    const changes = watchGitDir(repo, controller.signal);

    const next = changes.next();
    soon(() => writeFileSync(join(path, ".git", "reftable", "tables.list"), "x"));
    expect(await next).toEqual({ value: ["refs"], done: false });

    controller.abort();
  });

  it("ends quietly when aborted while starting", async () => {
    createRepo("watched-abort");
    const repo = await repos.open("watched-abort");
    await settled();
    const ends = [watchGitDir, watchWorkingTree].map((watch) => {
      const controller = new AbortController();
      const next = watch(repo, controller.signal).next();
      controller.abort();
      return next;
    });
    expect(await Promise.all(ends)).toEqual([
      { value: undefined, done: true },
      { value: undefined, done: true },
    ]);
  });

  it("follows a linked worktree's own HEAD and index", { timeout: 10_000 }, async () => {
    const main = createRepo("watched-main");
    git(main, "commit", "-q", "--allow-empty", "-m", "First");
    const linked = join(root, "watched-linked");
    git(main, "worktree", "add", "-q", linked);
    paths.set("watched-linked", linked);
    const repo = await repos.open("watched-linked");
    await settled();
    const controller = new AbortController();
    const changes = watchGitDir(repo, controller.signal);

    let next = changes.next();
    soon(() => {
      writeFileSync(join(linked, "file.txt"), "x");
      git(linked, "add", "file.txt");
    });
    expect(await next).toEqual({ value: ["index"], done: false });

    // The main worktree's index isn't this one's.
    next = changes.next();
    writeFileSync(join(main, "other.txt"), "x");
    git(main, "add", "other.txt");
    expect(await quiet(next)).toBe("quiet");

    // Branches are shared.
    git(main, "branch", "shared");
    expect((await next).value).toContain("refs");

    controller.abort();
  });
});
