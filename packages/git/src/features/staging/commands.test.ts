import { execFileSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  DiscardBlockedError,
  IndexLockedError,
  LinesNotStageableError,
  PatchChangedError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createMergeConflict } from "../../test/conflicts";
import { createHistoryRepo, createRepo, git, rejection, repos } from "../../test/fixtures";
import { getStagedFilePatch, getUnstagedFilePatch } from "../diff/commands";
import { getStatus } from "../status/commands";
import { parseStatus, STATUS_ARGS } from "../status/parse";
import {
  discard,
  discardAll,
  stage,
  stageAll,
  stageLines,
  unstage,
  unstageAll,
  unstageLines,
} from "./commands";
import type { LineSelection } from "./schema";

/** Each changed file, with whether its change is staged and unstaged. */
async function statusFiles(repo: Repo) {
  return parseStatus(await repo.read(STATUS_ARGS)).files;
}

describe("staging files", () => {
  it("moves staged changes between the working tree and the index", async () => {
    const repo = await createHistoryRepo();
    const change = {
      path: "a file.txt",
      status: "modified",
      origPath: null,
      additions: 1,
      deletions: 2,
    };
    const untrackedFile = {
      path: "new file.txt",
      status: "untracked",
      origPath: null,
      additions: null,
      deletions: null,
    };

    await stage(repo, ["a file.txt"]);
    expect((await getStatus(repo)).changes).toEqual({
      staged: [change],
      unstaged: [untrackedFile],
      uncounted: false,
      markerFree: [],
    });

    await unstage(repo, ["a file.txt"]);
    expect((await getStatus(repo)).changes).toEqual({
      staged: [],
      unstaged: [change, untrackedFile],
      uncounted: false,
      markerFree: [],
    });
  });

  it("stages and unstages before the first commit", async () => {
    const path = createRepo("empty");
    const repo = await repos.open("empty");
    writeFileSync(join(path, "x y.txt"), "hi\n");

    await stage(repo, ["x y.txt"]);
    expect(await statusFiles(repo)).toEqual([
      { path: "x y.txt", origPath: null, staged: "added", unstaged: null },
    ]);
    expect((await getStatus(repo)).changes).toEqual({
      staged: [{ path: "x y.txt", status: "added", origPath: null, additions: 1, deletions: 0 }],
      unstaged: [],
      uncounted: false,
      markerFree: [],
    });

    await unstage(repo, ["x y.txt"]);
    expect(await statusFiles(repo)).toEqual([
      { path: "x y.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("recognises another git process holding the index", async () => {
    const path = createRepo("locked");
    const repo = await repos.open("locked");
    writeFileSync(join(path, "file.txt"), "x");
    writeFileSync(join(path, ".git", "index.lock"), "");
    await expect(stage(repo, ["file.txt"])).rejects.toBeInstanceOf(IndexLockedError);
  });
});

describe("a big working tree", () => {
  it("stages and unstages more paths than fit on a Windows command line", async () => {
    const path = createRepo("big-stage");
    const names = Array.from({ length: 1000 }, (_, i) => `${"long-file-name-".repeat(3)}${i}.txt`);
    for (const name of names) writeFileSync(join(path, name), "a\n");
    const repo = await repos.open("big-stage");

    await stage(repo, names);
    expect((await getStatus(repo)).changes.staged).toHaveLength(1000);
    await unstage(repo, names);
    expect((await getStatus(repo)).changes).toMatchObject({ staged: [] });
  });

  it("stages and unstages everything at once", async () => {
    const path = createRepo("all");
    for (const name of ["kept.txt", "deleted.txt", "moved.txt"]) {
      writeFileSync(join(path, name), `${name}\n`);
    }
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    writeFileSync(join(path, "kept.txt"), "changed\n");
    rmSync(join(path, "deleted.txt"));
    git(path, "mv", "moved.txt", "renamed.txt");
    writeFileSync(join(path, "new file.txt"), "new\n");
    const repo = await repos.open("all");

    await stageAll(repo);
    expect(
      (await statusFiles(repo)).map((file) => [file.path, file.staged, file.unstaged]),
    ).toEqual([
      ["deleted.txt", "deleted", null],
      ["kept.txt", "modified", null],
      ["new file.txt", "added", null],
      ["renamed.txt", "renamed", null],
    ]);

    await unstageAll(repo);
    expect((await getStatus(repo)).changes.staged).toEqual([]);
    expect((await statusFiles(repo)).map((file) => [file.path, file.unstaged])).toEqual([
      ["deleted.txt", "deleted"],
      ["kept.txt", "modified"],
      ["moved.txt", "deleted"],
      ["new file.txt", "untracked"],
      ["renamed.txt", "untracked"],
    ]);
  });

  it("unstages everything but conflicts, which stay conflicted", async () => {
    const path = createRepo("all-conflict");
    writeFileSync(join(path, "both.txt"), "base\n");
    writeFileSync(join(path, "other.txt"), "other\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "base");
    git(path, "checkout", "-q", "-b", "side");
    writeFileSync(join(path, "both.txt"), "side\n");
    git(path, "commit", "-q", "-am", "side");
    git(path, "checkout", "-q", "main");
    writeFileSync(join(path, "both.txt"), "main\n");
    git(path, "commit", "-q", "-am", "main");
    expect(() => git(path, "merge", "-q", "side")).toThrow();
    writeFileSync(join(path, "other.txt"), "changed\n");
    git(path, "add", "other.txt");
    const repo = await repos.open("all-conflict");

    await unstageAll(repo);
    expect(await statusFiles(repo)).toEqual([
      { path: "other.txt", origPath: null, staged: null, unstaged: "modified" },
      {
        path: "both.txt",
        origPath: null,
        staged: "conflicted",
        unstaged: "conflicted",
        conflict: { xy: "UU", ours: "100644", theirs: "100644" },
      },
    ]);
    expect(existsSync(join(path, ".git", "MERGE_HEAD"))).toBe(true);
  });

  it("unstages a folder that replaced a file, a symlink and a file made executable", async () => {
    const path = createRepo("all-kinds");
    writeFileSync(join(path, "thing"), "a file\n");
    writeFileSync(join(path, "script.sh"), "echo\n");
    writeFileSync(join(path, "tab\tand\nline.txt"), "odd name\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    rmSync(join(path, "thing"));
    mkdirSync(join(path, "thing"));
    writeFileSync(join(path, "thing", "inner.txt"), "now a folder\n");
    symlinkSync("script.sh", join(path, "link"));
    git(path, "update-index", "--chmod=+x", "script.sh");
    writeFileSync(join(path, "tab\tand\nline.txt"), "changed\n");
    git(path, "add", ".");
    const repo = await repos.open("all-kinds");
    const head = git(path, "ls-tree", "-r", "HEAD");

    await unstageAll(repo);
    expect((await getStatus(repo)).changes.staged).toEqual([]);
    // The index is HEAD's again: modes, objects and all.
    expect(git(path, "ls-files", "--stage").replaceAll(" 0\t", "\t")).toBe(
      head.replaceAll(" blob ", " "),
    );
    expect(readFileSync(join(path, "thing", "inner.txt"), "utf8")).toBe("now a folder\n");
  });

  it("unstages a file whose name isn't UTF-8", async () => {
    const path = createRepo("all-latin1");
    // "café.txt" in Latin-1: read as UTF-8, its é doesn't come back as the byte it was.
    const name = Buffer.concat([Buffer.from("caf"), Buffer.from([0xe9]), Buffer.from(".txt")]);
    const file = Buffer.concat([Buffer.from(`${path}/`), name]);
    writeFileSync(file, "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    writeFileSync(file, "changed\n");
    git(path, "add", ".");
    const repo = await repos.open("all-latin1");

    await unstageAll(repo);
    // Nothing staged, and no entry under another name.
    expect(git(path, "diff", "--cached", "--name-only")).toBe("");
    expect(git(path, "ls-files", "-z").split("\0").filter(Boolean)).toHaveLength(1);
    // The change is still there, unstaged.
    expect(git(path, "diff", "--name-only")).not.toBe("");
  });

  it("unstages everything without giving git the staged files as paths to look up", async () => {
    const path = createRepo("all-unlisted");
    writeFileSync(join(path, "a.txt"), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    writeFileSync(join(path, "a.txt"), "changed\n");
    writeFileSync(join(path, "b.txt"), "b\n");
    git(path, "add", ".");
    const repo = await repos.open("all-unlisted");

    // Git looks every file in the index up in such a list: seconds, for thousands of them.
    const commands: string[] = [];
    const watched: Repo = {
      ...repo,
      write: (args, options) => {
        commands.push(args.join(" "));
        return repo.write(args, options);
      },
      exclusive: (task) =>
        repo.exclusive((run) =>
          task((args, options) => {
            commands.push(args.join(" "));
            return run(args, options);
          }),
        ),
    };
    await unstageAll(watched);
    expect(commands.filter((command) => command.includes("pathspec"))).toEqual([]);
    expect(await statusFiles(repo)).toEqual([
      { path: "a.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "b.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("stages and unstages everything before the first commit", async () => {
    const path = createRepo("all-empty");
    const repo = await repos.open("all-empty");
    await unstageAll(repo);

    writeFileSync(join(path, "a.txt"), "a\n");
    await stageAll(repo);
    expect((await getStatus(repo)).changes.staged).toHaveLength(1);
    await unstageAll(repo);
    expect(await statusFiles(repo)).toEqual([
      { path: "a.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("reports a locked index when staging more paths than git reads before it fails", async () => {
    const path = createRepo("big-locked");
    const repo = await repos.open("big-locked");
    writeFileSync(join(path, ".git", "index.lock"), "");
    // About 1MB: more than the pipe holds, so git exits before taking it all from stdin.
    const names = Array.from({ length: 20_000 }, (_, i) => `${"long-file-name-".repeat(3)}${i}`);
    await expect(stage(repo, names)).rejects.toBeInstanceOf(IndexLockedError);
  });
});

/** A file's unstaged and staged patches, as the UI has them. */
const unstagedPatch = (
  repo: Repo,
  path: string,
  untracked = false,
  origPath: string | null = null,
) => getUnstagedFilePatch(repo, { path, origPath, untracked });
const stagedPatch = (repo: Repo, path: string, origPath: string | null = null) =>
  getStagedFilePatch(repo, { path, origPath });

/** Every changed line of `patch`: removed ones by their old number, added ones by their new one. */
function allLines(patch: string): LineSelection {
  const selection: LineSelection = { deletions: [], additions: [] };
  let oldLine = 0;
  let newLine = 0;
  for (const line of patch.split("\n")) {
    const header = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(line);
    if (header) {
      oldLine = Number(header[1]);
      newLine = Number(header[2]);
    } else if (line.startsWith(" ")) {
      oldLine++;
      newLine++;
    } else if (line.startsWith("-") && !line.startsWith("--- ")) {
      selection.deletions.push({ start: oldLine, end: oldLine++ });
    } else if (line.startsWith("+") && !line.startsWith("+++ ")) {
      selection.additions.push({ start: newLine, end: newLine++ });
    }
  }
  return selection;
}

/** Single lines: removed ones by their old number, added ones by their new one. */
const lines = (deletions: number[], additions: number[]): LineSelection => ({
  deletions: deletions.map((line) => ({ start: line, end: line })),
  additions: additions.map((line) => ({ start: line, end: line })),
});

/** A file as the index has it. */
const indexed = (path: string, file: string) => git(path, "show", `:${file}`);

/** `text`'s bytes in Latin-1, as a file that isn't UTF-8 has them. */
const latin1 = (text: string) => Buffer.from(text, "latin1");

describe("staging lines", () => {
  it("stages lines of one hunk, leaving the rest unstaged", async () => {
    const path = createRepo("lines-hunks");
    const before = Array.from({ length: 20 }, (_, i) => `${i + 1}`);
    writeFileSync(join(path, "f.txt"), `${before.join("\n")}\n`);
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    const after = before.map((line) =>
      line === "3" ? "three" : line === "17" ? "seventeen" : line,
    );
    writeFileSync(join(path, "f.txt"), `${after.join("\n")}\n`);
    const repo = await repos.open("lines-hunks");

    const shown = await unstagedPatch(repo, "f.txt");
    // The second hunk: 17 replaced.
    const left = await stageLines(
      repo,
      { path: "f.txt", origPath: null, untracked: false },
      shown,
      lines([17], [17]),
    );
    expect(indexed(path, "f.txt").split("\n")[16]).toBe("seventeen");
    expect(indexed(path, "f.txt").split("\n")[2]).toBe("3");
    // The unstaged changes left, as a fresh read has them.
    expect(left).toBe(await unstagedPatch(repo, "f.txt"));
    expect(left).toContain("-3\n+three\n");
    expect(left).not.toContain("seventeen");

    // And back.
    const staged = await stagedPatch(repo, "f.txt");
    expect(
      await unstageLines(repo, { path: "f.txt", origPath: null }, staged, allLines(staged)),
    ).toBe("");
    expect(git(path, "diff", "--cached", "--name-only")).toBe("");
  });

  it("stages nothing from a patch that isn't the file's any more", async () => {
    const path = createRepo("lines-stale");
    writeFileSync(join(path, "f.txt"), "a\nb\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "f.txt"), "a\nB\n");
    const repo = await repos.open("lines-stale");
    const shown = await unstagedPatch(repo, "f.txt");
    const file = { path: "f.txt", origPath: null, untracked: false };

    // Saved again since: the lines picked were of other changes.
    writeFileSync(join(path, "f.txt"), "A\nB\n");
    const error = await rejection(stageLines(repo, file, shown, lines([2], [2])));
    expect(error).toBeInstanceOf(PatchChangedError);
    expect(error).toMatchObject({
      message: "The file changed since its changes were shown, so nothing was staged.",
    });
    // Staged since, in full: no changes left.
    git(path, "add", ".");
    expect(await rejection(stageLines(repo, file, shown, lines([2], [2])))).toBeInstanceOf(
      PatchChangedError,
    );
    expect(indexed(path, "f.txt")).toBe("A\nB");

    // Unstaging, the same.
    const staged = await stagedPatch(repo, "f.txt");
    git(path, "reset", "-q");
    expect(
      await rejection(
        unstageLines(repo, { path: "f.txt", origPath: null }, staged, allLines(staged)),
      ),
    ).toBeInstanceOf(PatchChangedError);
  });

  it("says when none of the lines picked are changes", async () => {
    const path = createRepo("lines-none");
    writeFileSync(join(path, "f.txt"), "a\nb\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "f.txt"), "a\nB\n");
    const repo = await repos.open("lines-none");
    const shown = await unstagedPatch(repo, "f.txt");
    const file = { path: "f.txt", origPath: null, untracked: false };
    expect(await rejection(stageLines(repo, file, shown, lines([1], [1])))).toBeInstanceOf(
      LinesNotStageableError,
    );
  });

  it("stages some lines of an untracked file as a new file", async () => {
    const path = createRepo("lines-untracked");
    writeFileSync(join(path, "x y.txt"), "a\nb\nc\n");
    git(path, "commit", "-q", "--allow-empty", "-m", "first");
    const repo = await repos.open("lines-untracked");
    const file = { path: "x y.txt", origPath: null, untracked: true };

    const left = await stageLines(
      repo,
      file,
      await unstagedPatch(repo, "x y.txt", true),
      lines([], [1, 3]),
    );
    expect(indexed(path, "x y.txt")).toBe("a\nc");
    // Compared to what's staged of it now.
    expect(left).toBe(await unstagedPatch(repo, "x y.txt", false));
    expect(left).toContain("@@ -1,2 +1,3 @@\n a\n+b\n c\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "x y.txt", origPath: null, staged: "added", unstaged: "modified" },
    ]);
  });

  it("stages some lines of a file added with --intent-to-add", async () => {
    const path = createRepo("lines-intent");
    git(path, "commit", "-q", "--allow-empty", "-m", "first");
    writeFileSync(join(path, "f.txt"), "a\nb\n");
    git(path, "add", "-N", "f.txt");
    const repo = await repos.open("lines-intent");
    const file = { path: "f.txt", origPath: null, untracked: false };

    await stageLines(repo, file, await unstagedPatch(repo, "f.txt"), lines([], [2]));
    expect(indexed(path, "f.txt")).toBe("b");
  });

  it("unstages some lines of a new file, and all of them back to untracked", async () => {
    const path = createRepo("lines-added");
    writeFileSync(join(path, "f.txt"), "a\nb\nc\n");
    git(path, "add", ".");
    const repo = await repos.open("lines-added");
    const file = { path: "f.txt", origPath: null };

    // Before the first commit: the staged changes are compared to nothing.
    await unstageLines(repo, file, await stagedPatch(repo, "f.txt"), lines([], [2]));
    expect(indexed(path, "f.txt")).toBe("a\nc");
    const staged = await stagedPatch(repo, "f.txt");
    await unstageLines(repo, file, staged, allLines(staged));
    expect(await statusFiles(repo)).toEqual([
      { path: "f.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("stages some lines of a deleted file, keeping the rest", async () => {
    const path = createRepo("lines-deleted");
    writeFileSync(join(path, "f.txt"), "a\nb\nc\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    rmSync(join(path, "f.txt"));
    const repo = await repos.open("lines-deleted");
    const file = { path: "f.txt", origPath: null, untracked: false };

    await stageLines(repo, file, await unstagedPatch(repo, "f.txt"), lines([2], []));
    expect(indexed(path, "f.txt")).toBe("a\nc");
    const rest = await unstagedPatch(repo, "f.txt");
    await stageLines(repo, file, rest, allLines(rest));
    expect(await statusFiles(repo)).toEqual([
      { path: "f.txt", origPath: null, staged: "deleted", unstaged: null },
    ]);

    // Unstaging a line of the deletion puts it back in the index, alone.
    const staged = await stagedPatch(repo, "f.txt");
    await unstageLines(repo, { path: "f.txt", origPath: null }, staged, lines([3], []));
    expect(indexed(path, "f.txt")).toBe("c");
  });

  it("unstages lines of a staged rename, which stays staged", async () => {
    const path = createRepo("lines-renamed");
    const text = Array.from({ length: 10 }, (_, i) => `line ${i}`).join("\n");
    writeFileSync(join(path, "old name.txt"), `${text}\n`);
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    git(path, "mv", "old name.txt", "new name.txt");
    writeFileSync(join(path, "new name.txt"), `${text.replace("line 5", "line five")}\n`);
    git(path, "add", ".");
    const repo = await repos.open("lines-renamed");

    const staged = await stagedPatch(repo, "new name.txt", "old name.txt");
    await unstageLines(
      repo,
      { path: "new name.txt", origPath: "old name.txt" },
      staged,
      allLines(staged),
    );
    expect(await statusFiles(repo)).toEqual([
      { path: "new name.txt", origPath: "old name.txt", staged: "renamed", unstaged: "modified" },
    ]);
    expect(indexed(path, "new name.txt")).toBe(text);
  });

  it("stages lines of a rename added with --intent-to-add with the rename", async () => {
    const path = createRepo("lines-intent-rename");
    const text = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
    writeFileSync(join(path, "old.txt"), `${text}\n`);
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    rmSync(join(path, "old.txt"));
    const changed = text.replace("line 2", "line two").replace("line 17", "line seventeen");
    writeFileSync(join(path, "new.txt"), `${changed}\n`);
    git(path, "add", "-N", "new.txt");
    const repo = await repos.open("lines-intent-rename");
    const file = { path: "new.txt", origPath: "old.txt", untracked: false };

    const shown = await unstagedPatch(repo, "new.txt", false, "old.txt");
    expect(shown).toContain("rename from old.txt");
    await stageLines(repo, file, shown, lines([3], [3]));
    expect(await statusFiles(repo)).toEqual([
      { path: "new.txt", origPath: "old.txt", staged: "renamed", unstaged: "modified" },
    ]);
    expect(indexed(path, "new.txt")).toBe(text.replace("line 2", "line two"));
  });

  it("leaves a change of mode where it is", async () => {
    const path = createRepo("lines-mode");
    writeFileSync(join(path, "run.sh"), "echo a\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "run.sh"), "echo b\n");
    chmodSync(join(path, "run.sh"), 0o755);
    const repo = await repos.open("lines-mode");
    const file = { path: "run.sh", origPath: null, untracked: false };

    const shown = await unstagedPatch(repo, "run.sh");
    await stageLines(repo, file, shown, allLines(shown));
    expect(git(path, "ls-files", "-s", "run.sh")).toMatch(/^100644 /);
    expect(indexed(path, "run.sh")).toBe("echo b");
    expect(await unstagedPatch(repo, "run.sh")).toBe(
      "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n",
    );
  });

  it("can't stage a binary file's changes by line", async () => {
    const path = createRepo("lines-binary");
    writeFileSync(join(path, "b.dat"), Buffer.from([0, 1, 2]));
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "b.dat"), Buffer.from([0, 1, 3]));
    const repo = await repos.open("lines-binary");
    const file = { path: "b.dat", origPath: null, untracked: false };

    const error = await rejection(
      stageLines(repo, file, await unstagedPatch(repo, "b.dat"), lines([1], [1])),
    );
    expect(error).toBeInstanceOf(LinesNotStageableError);
    expect(error).toMatchObject({ message: "A binary file's changes can only be staged whole." });
  });

  it("stages lines of a file that isn't UTF-8 as they are", async () => {
    const path = createRepo("lines-latin1");
    // "café" in Latin-1: read as UTF-8, its é doesn't come back as the byte it was.
    writeFileSync(join(path, "f.txt"), latin1("caf\xe9\nb\n"));
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "f.txt"), latin1("caf\xe9\nb\ncr\xe8me\nd\n"));
    const repo = await repos.open("lines-latin1");
    const file = { path: "f.txt", origPath: null, untracked: false };

    await stageLines(repo, file, await unstagedPatch(repo, "f.txt"), lines([], [3]));
    const index = execFileSync("git", ["show", ":f.txt"], { cwd: path });
    expect(index.equals(latin1("caf\xe9\nb\ncr\xe8me\n"))).toBe(true);
  });

  it.each([
    ["false", "\r\n"],
    ["true", "\n"],
  ])(
    "stages lines of a file with CRLF line ends as git has them in the index, with core.autocrlf %s",
    async (autocrlf, end) => {
      const name = `lines-crlf-${autocrlf}`;
      const path = createRepo(name);
      git(path, "config", "core.autocrlf", autocrlf);
      writeFileSync(join(path, "f.txt"), "a\r\nb\r\n");
      git(path, "add", ".");
      git(path, "commit", "-qm", "first");
      writeFileSync(join(path, "f.txt"), "a\r\nb\r\nc\r\nd\r\n");
      const repo = await repos.open(name);
      const file = { path: "f.txt", origPath: null, untracked: false };

      await stageLines(repo, file, await unstagedPatch(repo, "f.txt"), lines([], [4]));
      expect(execFileSync("git", ["show", ":f.txt"], { cwd: path, encoding: "utf8" })).toBe(
        `a${end}b${end}d${end}`,
      );
    },
  );

  it("stages lines as they are, whatever the user's settings for whitespace say", async () => {
    const path = createRepo("lines-whitespace");
    git(path, "config", "apply.whitespace", "error");
    git(path, "config", "apply.ignoreWhitespace", "change");
    writeFileSync(join(path, "f.txt"), "a\nb\n");
    git(path, "add", ".");
    git(path, "commit", "-qm", "first");
    writeFileSync(join(path, "f.txt"), "a  \nb\ntrailing  \n");
    const repo = await repos.open("lines-whitespace");
    const file = { path: "f.txt", origPath: null, untracked: false };

    await stageLines(repo, file, await unstagedPatch(repo, "f.txt"), lines([], [3]));
    expect(execFileSync("git", ["show", ":f.txt"], { cwd: path, encoding: "utf8" })).toBe(
      "a\nb\ntrailing  \n",
    );
  });
});

/** A seeded pseudo-random number generator (mulberry32), so a failure can be run again. */
function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Some of `selection`'s lines, each with a chance of one in two; `undefined` if that's none,
 * which there's nothing to stage of.
 */
function some(selection: LineSelection, next: () => number): LineSelection | undefined {
  const deletions = selection.deletions.filter(() => next() < 0.5);
  const additions = selection.additions.filter(() => next() < 0.5);
  return deletions.length + additions.length > 0 ? { deletions, additions } : undefined;
}

/**
 * A file of `base`'s lines, some edited, removed or added, with or without a newline at its end,
 * and with CRLF line ends or not.
 */
function someFile(next: () => number, base: string[], crlf: boolean): string {
  const kept: string[] = [];
  for (const line of base) {
    const roll = next();
    if (roll < 0.1) continue;
    if (roll < 0.2) kept.push(`${line} edited`);
    else kept.push(line);
    if (next() < 0.1) kept.push(`added after ${line}`);
  }
  if (next() < 0.2) kept.unshift("added first");
  const end = crlf ? "\r\n" : "\n";
  const text = kept.join(end);
  return next() < 0.3 || text === "" ? text : `${text}${end}`;
}

describe("staging lines, in any combination", () => {
  // A dozen files, about 12 git commands each: a few seconds. Passed for 400 when it was written.
  it(
    "stages some lines, then the rest, to the working tree's file; and unstages them back",
    { timeout: 30_000 },
    async () => {
      const path = createRepo("lines-random");
      const repo = await repos.open("lines-random");
      git(path, "commit", "-q", "--allow-empty", "-m", "first");
      const staged = { path: "f.txt", origPath: null };
      for (let seed = 1; seed <= 12; seed++) {
        const next = random(seed);
        const crlf = next() < 0.2;
        const base = Array.from({ length: 5 + Math.floor(next() * 40) }, (_, i) => `line ${i}`);
        const before = someFile(next, base, crlf);
        const after = someFile(next, base, crlf);
        if (before === after) continue;
        writeFileSync(join(path, "f.txt"), before);
        git(path, "add", ".");
        git(path, "commit", "-q", "--allow-empty", "-m", `seed ${seed}`);
        writeFileSync(join(path, "f.txt"), after);
        const context = `seed ${seed}`;

        // oxlint-disable no-await-in-loop -- each step builds on the last.
        const unstaged = { ...staged, untracked: false };
        const shown = await unstagedPatch(repo, "f.txt");
        const picked = some(allLines(shown), next);
        if (picked) await stageLines(repo, unstaged, shown, picked);
        const rest = await unstagedPatch(repo, "f.txt");
        if (rest) await stageLines(repo, unstaged, rest, allLines(rest));
        expect(git(path, "diff", "--name-only"), context).toBe("");

        const added = await stagedPatch(repo, "f.txt");
        const unpicked = some(allLines(added), next);
        if (unpicked) await unstageLines(repo, staged, added, unpicked);
        const left = await stagedPatch(repo, "f.txt");
        if (left) await unstageLines(repo, staged, left, allLines(left));
        // oxlint-enable no-await-in-loop
        expect(git(path, "diff", "--cached", "--name-only"), context).toBe("");
      }
    },
  );
});

/**
 * A repository, which `repos` opens as `name`, with a submodule (a repository inside it, committed
 * as one) at `mod`, which has a commit since.
 */
function createSubmoduleRepo(name: string): string {
  const path = createRepo(name);
  writeFileSync(join(path, "a.txt"), "a\n");
  const mod = join(path, "mod");
  mkdirSync(mod);
  git(mod, "init", "-q");
  git(mod, "-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "--allow-empty", "-m", "1");
  git(path, "add", ".");
  git(path, "commit", "-q", "-m", "first");
  git(mod, "-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "--allow-empty", "-m", "2");
  return path;
}

/** A file in the repository at `path`, read as text; `null` if there's none. */
function contents(path: string, file: string): string | null {
  return existsSync(join(path, file)) ? readFileSync(join(path, file), "utf8") : null;
}

describe("discarding a file's changes", () => {
  it("discards its unstaged changes, keeping its staged ones", async () => {
    const repo = await createHistoryRepo("discard-unstaged");
    const path = repo.path;
    writeFileSync(join(path, "a file.txt"), "a\nmore\nstaged\n");
    git(path, "add", "a file.txt");
    writeFileSync(join(path, "a file.txt"), "a\nmore\nstaged\nunstaged\n");

    await discard(repo, { path: "a file.txt", origPath: null }, "unstaged");
    expect(contents(path, "a file.txt")).toBe("a\nmore\nstaged\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: "modified", unstaged: null },
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("deletes an untracked file, and only it", async () => {
    const repo = await createHistoryRepo("discard-untracked");
    const path = repo.path;
    mkdirSync(join(path, "dir"));
    writeFileSync(join(path, "dir", "one.txt"), "one\n");
    writeFileSync(join(path, "dir", "two.txt"), "two\n");

    await discard(repo, { path: "dir/one.txt", origPath: null }, "unstaged");
    expect(contents(path, "dir/one.txt")).toBeNull();
    expect(contents(path, "dir/two.txt")).toBe("two\n");
    expect(contents(path, "new file.txt")).toBe("new\n");
  });

  it("brings back a file deleted in the working tree", async () => {
    const repo = await createHistoryRepo("discard-deleted");
    rmSync(join(repo.path, "bin.dat"));

    await discard(repo, { path: "bin.dat", origPath: null }, "unstaged");
    expect(readFileSync(join(repo.path, "bin.dat"))).toEqual(Buffer.from([0, 1, 2]));
  });

  it("discards all of them from its staged changes, back to how HEAD has it", async () => {
    const repo = await createHistoryRepo("discard-staged");
    const path = repo.path;
    writeFileSync(join(path, "a file.txt"), "staged\n");
    git(path, "add", "a file.txt");
    writeFileSync(join(path, "a file.txt"), "staged\nunstaged\n");

    await discard(repo, { path: "a file.txt", origPath: null }, "staged");
    expect(contents(path, "a file.txt")).toBe("a\nmore\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("deletes a staged new file, and puts back a staged rename", async () => {
    const repo = await createHistoryRepo("discard-staged-new");
    const path = repo.path;
    git(path, "add", "new file.txt");
    git(path, "mv", "c.txt", "d.txt");

    await discard(repo, { path: "new file.txt", origPath: null }, "staged");
    await discard(repo, { path: "d.txt", origPath: "c.txt" }, "staged");
    expect(contents(path, "new file.txt")).toBeNull();
    expect(contents(path, "d.txt")).toBeNull();
    expect(contents(path, "c.txt")).toBe("b\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
    ]);
  });

  it("leaves a copy's source as it is, which is another file", async () => {
    const repo = await createHistoryRepo("discard-copy");
    const path = repo.path;
    writeFileSync(join(path, "copy.txt"), "a\nmore\n");
    git(path, "add", "copy.txt");
    writeFileSync(join(path, "untracked copy.txt"), "a\nmore\n");

    // As `git status` lists them with `status.renames=copies`.
    await discard(repo, { path: "copy.txt", origPath: "a file.txt" }, "staged");
    await discard(repo, { path: "untracked copy.txt", origPath: "a file.txt" }, "unstaged");
    expect(contents(path, "copy.txt")).toBeNull();
    expect(contents(path, "untracked copy.txt")).toBeNull();
    expect(contents(path, "a file.txt")).toBe("changed\n");
  });

  it("deletes a staged file before the first commit", async () => {
    const path = createRepo("discard-unborn");
    writeFileSync(join(path, "x y.txt"), "hi\n");
    writeFileSync(join(path, "kept.txt"), "kept\n");
    git(path, "add", ".");
    const repo = await repos.open("discard-unborn");

    await discard(repo, { path: "x y.txt", origPath: null }, "staged");
    expect(contents(path, "x y.txt")).toBeNull();
    expect(await statusFiles(repo)).toEqual([
      { path: "kept.txt", origPath: null, staged: "added", unstaged: null },
    ]);
  });

  it("deletes a file added with --intent-to-add, rather than emptying it", async () => {
    const repo = await createHistoryRepo("discard-intent");
    git(repo.path, "add", "-N", "new file.txt");

    await discard(repo, { path: "new file.txt", origPath: null }, "unstaged");
    expect(contents(repo.path, "new file.txt")).toBeNull();
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
    ]);
  });

  it("deletes a file added with --intent-to-add that git pairs with a deleted one as its rename", async () => {
    const repo = await createHistoryRepo("discard-intent-rename");
    const path = repo.path;
    renameSync(join(path, "c.txt"), join(path, "d.txt"));
    git(path, "add", "-N", "d.txt");

    await discard(repo, { path: "d.txt", origPath: "c.txt" }, "unstaged");
    expect(contents(path, "d.txt")).toBeNull();
    expect(contents(path, "c.txt")).toBe("b\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("leaves the files in a folder of the same name alone", async () => {
    const path = createRepo("discard-folder");
    mkdirSync(join(path, "foo"));
    writeFileSync(join(path, "foo", "a"), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    rmSync(join(path, "foo"), { recursive: true });
    writeFileSync(join(path, "foo"), "now a file\n");
    const repo = await repos.open("discard-folder");

    await discard(repo, { path: "foo", origPath: null }, "unstaged");
    expect(await statusFiles(repo)).toEqual([
      { path: "foo/a", origPath: null, staged: null, unstaged: "deleted" },
    ]);
  });

  it("doesn't put back a deleted file where a folder has taken its place", async () => {
    const path = createRepo("discard-folder-unstaged");
    writeFileSync(join(path, "a"), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    rmSync(join(path, "a"));
    mkdirSync(join(path, "a"));
    writeFileSync(join(path, "a", "x"), "precious\n");
    const repo = await repos.open("discard-folder-unstaged");

    const error = await rejection(discard(repo, { path: "a", origPath: null }, "unstaged"));
    expect(error).toBeInstanceOf(DiscardBlockedError);
    expect((error as Error).message).toBe(
      "A folder is at a now. Move or delete it, then discard the changes.",
    );
    expect(contents(path, "a/x")).toBe("precious\n");
  });

  it("doesn't put back a staged deletion over a folder with staged files in it", async () => {
    const path = createRepo("discard-folder-staged");
    writeFileSync(join(path, "a"), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    git(path, "rm", "-q", "a");
    mkdirSync(join(path, "a"));
    writeFileSync(join(path, "a", "x"), "precious\n");
    git(path, "add", "a/x");
    const repo = await repos.open("discard-folder-staged");

    expect(await rejection(discard(repo, { path: "a", origPath: null }, "staged"))).toBeInstanceOf(
      DiscardBlockedError,
    );
    expect(contents(path, "a/x")).toBe("precious\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "a", origPath: null, staged: "deleted", unstaged: null },
      { path: "a/x", origPath: null, staged: "added", unstaged: null },
    ]);
  });

  it("doesn't put back a rename's previous path over a new file there", async () => {
    const repo = await createHistoryRepo("discard-rename-taken");
    const path = repo.path;
    git(path, "mv", "c.txt", "d.txt");
    writeFileSync(join(path, "c.txt"), "new work\n");

    const error = await rejection(discard(repo, { path: "d.txt", origPath: "c.txt" }, "staged"));
    expect((error as Error).message).toBe(
      "The file at c.txt isn't the last commit's. Move or delete it, then discard the changes.",
    );
    expect(contents(path, "c.txt")).toBe("new work\n");
    expect(contents(path, "d.txt")).toBe("b\n");
  });

  it("discards no staged changes while a merge is under way, which would go on without them", async () => {
    const path = createMergeConflict("discard-merge");
    writeFileSync(join(path, "f.txt"), "one\nresolved\nthree\n");
    git(path, "add", "f.txt");
    writeFileSync(join(path, "README"), "changed\n");
    const repo = await repos.open("discard-merge");

    const error = await rejection(discard(repo, { path: "f.txt", origPath: null }, "staged"));
    expect((error as Error).message).toBe(
      "A merge is under way. Finish or abort it, then discard its staged changes.",
    );
    expect(contents(path, "f.txt")).toBe("one\nresolved\nthree\n");
    // Unstaged changes aren't the merge's.
    await discard(repo, { path: "README", origPath: null }, "unstaged");
    expect(contents(path, "README")).toBe("readme\n");
  });

  it("puts back a file taken out of the index over its copy, unless that changed", async () => {
    const repo = await createHistoryRepo("discard-rm-cached");
    const path = repo.path;
    git(path, "rm", "-q", "--cached", "a file.txt", "c.txt");

    // Changed since: its changes aren't listed as such, so they aren't written over.
    const error = await rejection(discard(repo, { path: "a file.txt", origPath: null }, "staged"));
    expect((error as Error).message).toBe(
      "The file at a file.txt isn't the last commit's. Move or delete it, then discard the changes.",
    );
    expect(contents(path, "a file.txt")).toBe("changed\n");

    await discard(repo, { path: "c.txt", origPath: null }, "staged");
    expect(contents(path, "c.txt")).toBe("b\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: "deleted", unstaged: null },
      { path: "a file.txt", origPath: null, staged: null, unstaged: "untracked" },
      { path: "new file.txt", origPath: null, staged: null, unstaged: "untracked" },
    ]);
  });

  it("deletes a file added with --intent-to-add that's been deleted, rather than emptying it", async () => {
    const repo = await createHistoryRepo("discard-intent-deleted");
    git(repo.path, "add", "-N", "new file.txt");
    rmSync(join(repo.path, "new file.txt"));

    await discard(repo, { path: "new file.txt", origPath: null }, "unstaged");
    expect(contents(repo.path, "new file.txt")).toBeNull();
    expect(await statusFiles(repo)).toEqual([
      { path: "a file.txt", origPath: null, staged: null, unstaged: "modified" },
    ]);
  });

  it("names the conflicted path, a rename's previous one too", async () => {
    const path = createMergeConflict("discard-conflict-orig");
    const repo = await repos.open("discard-conflict-orig");
    writeFileSync(join(path, "g.txt"), "g\n");
    git(path, "add", "g.txt");

    const error = await rejection(discard(repo, { path: "g.txt", origPath: "f.txt" }, "unstaged"));
    expect((error as Error).message).toMatch(/^f\.txt is conflicted\./);
  });

  it("says a file is in the way where one of the folders of the file to put back goes", async () => {
    const path = createRepo("discard-blocked");
    mkdirSync(join(path, "d"));
    writeFileSync(join(path, "d", "x"), "x\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    rmSync(join(path, "d"), { recursive: true });
    writeFileSync(join(path, "d"), "stuff\n");
    const repo = await repos.open("discard-blocked");

    const error = await rejection(discard(repo, { path: "d/x", origPath: null }, "unstaged"));
    expect((error as Error).message).toBe(
      "A file is where a folder of d/x goes. Move or delete it, then discard the changes.",
    );
    expect(contents(path, "d")).toBe("stuff\n");
  });

  it("leaves a submodule's changes to be discarded in it", async () => {
    createSubmoduleRepo("discard-submodule");
    const repo = await repos.open("discard-submodule");

    const error = await rejection(discard(repo, { path: "mod", origPath: null }, "unstaged"));
    expect((error as Error).message).toBe(
      "mod is a submodule, a repository of its own: discard its changes in it.",
    );
  });

  it("leaves a conflicted file to be resolved, on either side", async () => {
    const path = createMergeConflict("discard-conflict");
    const repo = await repos.open("discard-conflict");
    const before = contents(path, "f.txt");

    for (const side of ["unstaged", "staged"] as const) {
      // oxlint-disable-next-line no-await-in-loop -- one after the other, on the same file.
      const error = await rejection(discard(repo, { path: "f.txt", origPath: null }, side));
      expect(error).toBeInstanceOf(DiscardBlockedError);
    }
    expect(contents(path, "f.txt")).toBe(before);
    expect(git(path, "ls-files", "--unmerged")).not.toBe("");
  });
});

describe("discarding all changes", () => {
  it("puts back every file as HEAD has it and deletes untracked ones, but not ignored ones", async () => {
    const repo = await createHistoryRepo("discard-all");
    const path = repo.path;
    writeFileSync(join(path, ".git", "info", "exclude"), "*.log\n");
    writeFileSync(join(path, "debug.log"), "ignored\n");
    git(path, "add", "a file.txt", "new file.txt");
    writeFileSync(join(path, "a file.txt"), "changed again\n");
    git(path, "mv", "c.txt", "d.txt");
    rmSync(join(path, "bin.dat"));
    mkdirSync(join(path, "dir", "deeper"), { recursive: true });
    writeFileSync(join(path, "dir", "deeper", "untracked.txt"), "untracked\n");

    await discardAll(repo);
    expect(await statusFiles(repo)).toEqual([]);
    expect(contents(path, "a file.txt")).toBe("a\nmore\n");
    expect(contents(path, "c.txt")).toBe("b\n");
    expect(contents(path, "new file.txt")).toBeNull();
    expect(existsSync(join(path, "dir"))).toBe(false);
    expect(readFileSync(join(path, "bin.dat"))).toEqual(Buffer.from([0, 1, 2]));
    expect(contents(path, "debug.log")).toBe("ignored\n");
  });

  it("deletes a staged file that's ignored, which isn't kept like the unstaged ones", async () => {
    const repo = await createHistoryRepo("discard-all-ignored");
    const path = repo.path;
    writeFileSync(join(path, ".git", "info", "exclude"), "*.log\n");
    writeFileSync(join(path, "staged.log"), "staged\n");
    git(path, "add", "-f", "staged.log");

    await discardAll(repo);
    expect(contents(path, "staged.log")).toBeNull();
  });

  it("leaves a deleted file deleted where a folder took its place, with what's in it", async () => {
    const repo = await createHistoryRepo("discard-all-folder");
    const path = repo.path;
    writeFileSync(join(path, ".git", "info", "exclude"), "*.log\n");
    rmSync(join(path, "c.txt"));
    mkdirSync(join(path, "c.txt"));
    writeFileSync(join(path, "c.txt", "important.log"), "kept\n");

    await discardAll(repo);
    expect(contents(path, "c.txt/important.log")).toBe("kept\n");
    // The rest is discarded all the same.
    expect(await statusFiles(repo)).toEqual([
      { path: "c.txt", origPath: null, staged: null, unstaged: "deleted" },
    ]);
  });

  it("leaves a deleted folder's files deleted where an ignored file has taken its place", async () => {
    const path = createRepo("discard-all-file-for-folder");
    mkdirSync(join(path, "d"));
    writeFileSync(join(path, "d", "x"), "x\n");
    writeFileSync(join(path, "other.txt"), "other\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    writeFileSync(join(path, ".git", "info", "exclude"), "/d\n");
    rmSync(join(path, "d"), { recursive: true });
    writeFileSync(join(path, "d"), "stuff\n");
    writeFileSync(join(path, "other.txt"), "changed\n");
    const repo = await repos.open("discard-all-file-for-folder");

    await discardAll(repo);
    expect(contents(path, "d")).toBe("stuff\n");
    expect(contents(path, "other.txt")).toBe("other\n");
  });

  it("discards the staged files in a folder where a deleted file was, and puts the file back", async () => {
    const path = createRepo("discard-all-staged-folder");
    writeFileSync(join(path, "a"), "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    git(path, "rm", "-q", "a");
    mkdirSync(join(path, "a"));
    writeFileSync(join(path, "a", "x"), "x\n");
    git(path, "add", "a/x");
    const repo = await repos.open("discard-all-staged-folder");

    expect(await discardAll(repo)).toEqual([]);
    expect(contents(path, "a")).toBe("a\n");
    expect(await statusFiles(repo)).toEqual([]);
  });

  it("puts back a file taken out of the index that's ignored now, unless it changed", async () => {
    const repo = await createHistoryRepo("discard-all-untracked-ignored");
    const path = repo.path;
    writeFileSync(join(path, ".git", "info", "exclude"), "c.txt\nbin.dat\n");
    git(path, "rm", "-q", "--cached", "c.txt", "bin.dat");
    writeFileSync(join(path, "bin.dat"), "local\n");

    expect(await discardAll(repo)).toEqual(["bin.dat"]);
    expect(contents(path, "bin.dat")).toBe("local\n");
    expect(await statusFiles(repo)).toEqual([
      { path: "bin.dat", origPath: null, staged: "deleted", unstaged: null },
    ]);
  });

  it("leaves out a file something took the place of by its name alone, glob characters too", async () => {
    const path = createRepo("discard-all-glob-name");
    writeFileSync(join(path, "x[1]*"), "x\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    git(path, "rm", "-q", "x[1]*");
    mkdirSync(join(path, "x[1]*"));
    writeFileSync(join(path, "x[1]*", "y"), "y\n");
    git(path, "add", "x[1]*/y");
    const repo = await repos.open("discard-all-glob-name");

    expect(await discardAll(repo)).toEqual([]);
    expect(contents(path, "x[1]*")).toBe("x\n");
  });

  it("keeps a deleted file where a link, or a file named like a quoted path, took its place", async () => {
    const path = createRepo("discard-all-odd-copies");
    writeFileSync(join(path, "latest"), "l\n");
    writeFileSync(join(path, '"draft.txt'), "d\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    writeFileSync(join(path, ".git", "info", "exclude"), "latest\n");
    git(path, "rm", "-q", "--cached", "latest", '"draft.txt');
    rmSync(join(path, "latest"));
    mkdirSync(join(path, "v2"));
    symlinkSync("v2", join(path, "latest"));
    const repo = await repos.open("discard-all-odd-copies");

    // The quoted one is untracked, deleted, and put back; the link, ignored, is kept.
    expect(await discardAll(repo)).toEqual(["latest"]);
    expect(contents(path, '"draft.txt')).toBe("d\n");
  });

  it("keeps what only an uncommitted ignore rule ignores, which isn't listed", async () => {
    const repo = await createHistoryRepo("discard-all-new-ignore");
    const path = repo.path;
    writeFileSync(join(path, ".gitignore"), "secret.env\n");
    git(path, "add", ".gitignore");
    writeFileSync(join(path, "secret.env"), "secret\n");

    expect(await discardAll(repo)).toEqual([]);
    expect(contents(path, ".gitignore")).toBeNull();
    expect(contents(path, "secret.env")).toBe("secret\n");
    expect(contents(path, "new file.txt")).toBeNull();
  });

  it("doesn't say it kept folders of ignored files", async () => {
    const repo = await createHistoryRepo("discard-all-ignored-folder");
    writeFileSync(join(repo.path, ".git", "info", "exclude"), "*.pyc\n");
    mkdirSync(join(repo.path, "cache"));
    writeFileSync(join(repo.path, "cache", "a.pyc"), "pyc\n");

    expect(await discardAll(repo)).toEqual([]);
    expect(contents(repo.path, "cache/a.pyc")).toBe("pyc\n");
  });

  it("puts back a link taken out of the index, as it's unchanged", async () => {
    const path = createRepo("discard-all-link");
    mkdirSync(join(path, "v2"));
    writeFileSync(join(path, "v2", "f"), "f\n");
    symlinkSync("v2", join(path, "latest"));
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "first");
    // Ignored, so `clean` leaves it, and it's compared.
    writeFileSync(join(path, ".git", "info", "exclude"), "latest\n");
    git(path, "rm", "-q", "--cached", "latest");
    const repo = await repos.open("discard-all-link");

    expect(await discardAll(repo)).toEqual([]);
    expect(await statusFiles(repo)).toEqual([]);
  });

  it("deletes nothing untracked when it can't discard the rest", async () => {
    const repo = await createHistoryRepo("discard-all-locked");
    writeFileSync(join(repo.path, ".git", "index.lock"), "");

    expect(await rejection(discardAll(repo))).toBeInstanceOf(IndexLockedError);
    rmSync(join(repo.path, ".git", "index.lock"));
    expect(contents(repo.path, "new file.txt")).toBe("new\n");
  });

  it("before the first commit, deletes staged files but keeps a repository staged inside", async () => {
    const path = createRepo("discard-all-unborn-nested");
    writeFileSync(join(path, "f"), "f\n");
    const mod = join(path, "mod");
    mkdirSync(mod);
    git(mod, "init", "-q");
    git(
      mod,
      "-c",
      "user.name=T",
      "-c",
      "user.email=t@e",
      "commit",
      "-q",
      "--allow-empty",
      "-m",
      "1",
    );
    git(path, "add", "f", "mod");
    writeFileSync(join(path, "u"), "u\n");
    const repo = await repos.open("discard-all-unborn-nested");

    expect(await discardAll(repo)).toEqual(["mod/"]);
    expect(contents(path, "f")).toBeNull();
    expect(contents(path, "u")).toBeNull();
    expect(existsSync(join(mod, ".git"))).toBe(true);
  });

  it("says which changes it kept: a submodule's, and a repository's inside this one", async () => {
    const path = createSubmoduleRepo("discard-all-kept");
    const nested = join(path, "nested");
    mkdirSync(nested);
    git(nested, "init", "-q");
    writeFileSync(join(path, "a.txt"), "changed\n");
    const repo = await repos.open("discard-all-kept");

    expect(await discardAll(repo)).toEqual(["mod", "nested/"]);
    expect(contents(path, "a.txt")).toBe("a\n");
  });

  it("puts back a file whose name isn't UTF-8", async () => {
    const path = createRepo("discard-all-latin1");
    const name = Buffer.concat([Buffer.from("caf"), Buffer.from([0xe9]), Buffer.from(".txt")]);
    const file = Buffer.concat([Buffer.from(`${path}/`), name]);
    writeFileSync(file, "a\n");
    git(path, "add", ".");
    git(path, "commit", "-q", "-m", "First");
    writeFileSync(file, "changed\n");
    const repo = await repos.open("discard-all-latin1");

    await discardAll(repo);
    expect(readFileSync(file, "utf8")).toBe("a\n");
  });

  it("discards nothing while a merge is under way, which would be committed without them", async () => {
    const path = createMergeConflict("discard-all-merge");
    writeFileSync(join(path, "f.txt"), "one\nresolved\nthree\n");
    git(path, "add", "f.txt");
    const repo = await repos.open("discard-all-merge");

    const error = await rejection(discardAll(repo));
    expect(error).toBeInstanceOf(DiscardBlockedError);
    expect((error as Error).message).toBe(
      "A merge is under way. Finish or abort it, then discard the changes.",
    );
    expect(contents(path, "f.txt")).toBe("one\nresolved\nthree\n");
  });

  it("deletes files added with --intent-to-add", async () => {
    const repo = await createHistoryRepo("discard-all-intent");
    git(repo.path, "add", "-N", "new file.txt");

    await discardAll(repo);
    expect(await statusFiles(repo)).toEqual([]);
    expect(contents(repo.path, "new file.txt")).toBeNull();
  });

  it("discards nothing while files are conflicted", async () => {
    const path = createMergeConflict("discard-all-conflict");
    writeFileSync(join(path, "README"), "changed\n");
    const repo = await repos.open("discard-all-conflict");

    expect(await rejection(discardAll(repo))).toBeInstanceOf(DiscardBlockedError);
    expect(contents(path, "README")).toBe("changed\n");
    expect(git(path, "ls-files", "--unmerged")).not.toBe("");
  });

  it("deletes every file before the first commit", async () => {
    const path = createRepo("discard-all-unborn");
    writeFileSync(join(path, "staged.txt"), "staged\n");
    git(path, "add", ".");
    writeFileSync(join(path, "untracked.txt"), "untracked\n");
    const repo = await repos.open("discard-all-unborn");

    await discardAll(repo);
    expect(await statusFiles(repo)).toEqual([]);
    expect(contents(path, "staged.txt")).toBeNull();
  });

  it("deletes untracked files when no file was ever committed, and does nothing without changes", async () => {
    const path = createRepo("discard-all-empty");
    git(path, "commit", "-q", "--allow-empty", "-m", "empty");
    writeFileSync(join(path, "untracked.txt"), "untracked\n");
    const repo = await repos.open("discard-all-empty");

    await discardAll(repo);
    expect(contents(path, "untracked.txt")).toBeNull();
    await discardAll(repo);
    expect(await statusFiles(repo)).toEqual([]);
  });
});
