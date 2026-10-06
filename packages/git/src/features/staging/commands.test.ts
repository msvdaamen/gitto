import { execFileSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { IndexLockedError, LinesNotStageableError, PatchChangedError } from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createHistoryRepo, createRepo, git, rejection, repos } from "../../test/fixtures";
import { getStagedFilePatch, getUnstagedFilePatch } from "../diff/commands";
import { getStatus } from "../status/commands";
import { parseStatus, STATUS_ARGS } from "../status/parse";
import { stage, stageAll, stageLines, unstage, unstageAll, unstageLines } from "./commands";
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
