import { execFileSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import { beforeAll, describe, expect, it } from "vitest";

import {
  ChangesTooLargeError,
  FileTooLargeError,
  GitError,
  FileChangedOnDiskError,
  NotUtf8Error,
  OutsideRepositoryError,
  WorkingTreeFileNotFoundError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import {
  createHistoryRepo,
  createRepo,
  git,
  page,
  paths,
  rejection,
  repos,
} from "../../test/fixtures";
import { getLog } from "../history/commands";
import {
  getBlob,
  getCommitFilePatch,
  getCommitFiles,
  getStagedFilePatch,
  getUnstagedFilePatch,
} from "./commands";
import { MAX_BLOB_BYTES, MAX_PATCH_BYTES } from "./limits";
import { readWorkingTreeFile, saveWorkingTreeFile } from "./working-tree";

describe("getCommitFiles", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo();
  });

  it("diffs a merge against its first parent, and a root commit against nothing", async () => {
    const log = await getLog(repo, page);
    const merge = log.find((commit) => commit.subject === "Merge side");
    const first = log.find((commit) => commit.subject === "first");
    expect(await getCommitFiles(repo, merge!.sha)).toEqual([
      { path: "c.txt", status: "renamed", origPath: "b.txt", additions: 0, deletions: 0 },
      { path: "s.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
    ]);
    expect(await getCommitFiles(repo, first!.sha)).toEqual([
      { path: "a file.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "b.txt", status: "added", origPath: null, additions: 1, deletions: 0 },
      { path: "bin.dat", status: "added", origPath: null, additions: null, deletions: null },
    ]);
  });

  it("explains why a command failed", async () => {
    const error = await rejection(getCommitFiles(repo, "0123456789abcdef0123456789abcdef01234567"));
    expect(error).toBeInstanceOf(GitError);
    expect(error).toMatchObject({ message: expect.stringContaining("bad object") });
  });
});

describe("getCommitFilePatch", () => {
  let repo: Repo;

  beforeAll(async () => {
    repo = await createHistoryRepo("patches");
  });

  async function shaOf(subject: string) {
    const log = await getLog(repo, page);
    return log.find((commit) => commit.subject === subject)!.sha;
  }

  it("shows one file's changes, with its full object names", async () => {
    const patch = await getCommitFilePatch(repo, await shaOf("main commit"), {
      path: "a file.txt",
      origPath: null,
    });
    expect(patch).toMatch(
      /^diff --git a\/a file.txt b\/a file.txt\nindex [0-9a-f]{40}\.\.[0-9a-f]{40} 100644\n/,
    );
    expect(patch).toContain("@@ -1 +1,2 @@\n a\n+more\n");
  });

  it("finds a rename with its old path, compared to a merge's first parent", async () => {
    const patch = await getCommitFilePatch(repo, await shaOf("Merge side"), {
      path: "c.txt",
      origPath: "b.txt",
    });
    expect(patch).toContain("rename from b.txt\nrename to c.txt\n");
  });

  it("shows a root commit's files as added", async () => {
    const patch = await getCommitFilePatch(repo, await shaOf("first"), {
      path: "b.txt",
      origPath: null,
    });
    expect(patch).toContain("new file mode 100644\n");
    expect(patch).toContain("@@ -0,0 +1 @@\n+b\n");
  });
});

describe("getBlob", () => {
  it("reads a file by the object name on a patch's index line", async () => {
    const repo = await createHistoryRepo("blobs");
    const log = await getLog(repo, page);
    const sha = log.find((commit) => commit.subject === "main commit")!.sha;
    const patch = await getCommitFilePatch(repo, sha, { path: "a file.txt", origPath: null });
    const [, before, after] = /^index ([0-9a-f]+)\.\.([0-9a-f]+)/m.exec(patch)!;
    expect(await getBlob(repo, before!)).toBe("a\n");
    expect(await getBlob(repo, after!)).toBe("a\nmore\n");
  });

  it("won't read a file too large to send whole", async () => {
    const repo = await createHistoryRepo("large-blob");
    const oid = execFileSync("git", ["hash-object", "-w", "--stdin"], {
      cwd: paths.get("large-blob"),
      input: "x".repeat(MAX_BLOB_BYTES + 1),
      encoding: "utf8",
    }).trim();
    expect(await rejection(getBlob(repo, oid))).toBeInstanceOf(FileTooLargeError);
  });
});

describe("getUnstagedFilePatch", () => {
  it("compares the working tree with the index, ignoring the user's diff settings", async () => {
    const repo = await createHistoryRepo("unstaged");
    const path = paths.get("unstaged")!;
    git(path, "config", "diff.noprefix", "true");
    git(path, "config", "diff.external", "false");
    git(path, "config", "color.diff", "always");
    const patch = await getUnstagedFilePatch(repo, {
      path: "a file.txt",
      origPath: null,
      untracked: false,
    });
    expect(patch).toMatch(
      /^diff --git a\/a file.txt b\/a file.txt\nindex [0-9a-f]{40}\.\.[0-9a-f]{40} 100644\n/,
    );
    expect(patch).toContain("@@ -1,2 +1 @@\n-a\n-more\n+changed\n");
  });

  it("shows an untracked file as added, in full", async () => {
    const repo = await createHistoryRepo("untracked");
    const path = paths.get("untracked")!;
    git(path, "config", "diff.noprefix", "true");
    git(path, "config", "diff.mnemonicPrefix", "true");
    const patch = await getUnstagedFilePatch(repo, {
      path: "new file.txt",
      origPath: null,
      untracked: true,
    });
    expect(patch).toMatch(/^diff --git a\/new file.txt b\/new file.txt\nnew file mode 100644\n/);
    // A name with a space ends in a tab, as in other diffs.
    expect(patch).toContain("--- /dev/null\n+++ b/new file.txt\t\n@@ -0,0 +1 @@\n+new\n");
  });

  it("won't compare a path outside the working tree, but shows a link out as one", async () => {
    const repo = await createHistoryRepo("untracked-confined");
    const path = paths.get("untracked-confined")!;
    writeFileSync(join(path, "..", "outside.txt"), "secret\n");
    symlinkSync(join(path, "..", "outside.txt"), join(path, "link"));
    const outside = ["../outside.txt", "/etc/hosts", ".git/config", ".GIT/config"];
    const errors = await Promise.all(
      outside.map((name) =>
        rejection(getUnstagedFilePatch(repo, { path: name, origPath: null, untracked: true })),
      ),
    );
    for (const error of errors) expect(error).toBeInstanceOf(OutsideRepositoryError);
    const link = await getUnstagedFilePatch(repo, {
      path: "link",
      origPath: null,
      untracked: true,
    });
    expect(link).toContain("new file mode 120000\n");
    expect(link).not.toContain("secret");
  });

  it("says why an untracked path can't be compared, like another repository's folder", async () => {
    const repo = await createHistoryRepo("nested");
    mkdirSync(join(paths.get("nested")!, "inner"));
    const error = await rejection(
      getUnstagedFilePatch(repo, { path: "inner/", origPath: null, untracked: true }),
    );
    expect(error).toBeInstanceOf(GitError);
  });

  it("is empty for a file without unstaged changes", async () => {
    const repo = await createHistoryRepo("clean");
    expect(
      await getUnstagedFilePatch(repo, { path: "b.txt", origPath: null, untracked: false }),
    ).toBe("");
  });

  it("refuses changes too large to send", async () => {
    const repo = await createHistoryRepo("huge");
    writeFileSync(join(paths.get("huge")!, "huge.log"), "line\n".repeat(MAX_PATCH_BYTES / 4));
    const error = await rejection(
      getUnstagedFilePatch(repo, { path: "huge.log", origPath: null, untracked: true }),
    );
    expect(error).toBeInstanceOf(ChangesTooLargeError);
  });
});

describe("getStagedFilePatch", () => {
  it("compares the index with HEAD, finding renames", async () => {
    const repo = await createHistoryRepo("staged");
    const path = paths.get("staged")!;
    git(path, "add", "a file.txt");
    git(path, "mv", "c.txt", "d.txt");
    expect(await getStagedFilePatch(repo, { path: "a file.txt", origPath: null })).toContain(
      "@@ -1,2 +1 @@\n-a\n-more\n+changed\n",
    );
    expect(await getStagedFilePatch(repo, { path: "d.txt", origPath: "c.txt" })).toContain(
      "rename from c.txt\nrename to d.txt\n",
    );
    expect(await getStagedFilePatch(repo, { path: "s.txt", origPath: null })).toBe("");
  });

  it("compares with nothing before the first commit", async () => {
    const path = createRepo("unborn");
    const repo = await repos.open("unborn");
    writeFileSync(join(path, "first.txt"), "first\n");
    git(path, "add", "first.txt");
    const patch = await getStagedFilePatch(repo, { path: "first.txt", origPath: null });
    expect(patch).toContain("new file mode 100644\n");
    expect(patch).toContain("@@ -0,0 +1 @@\n+first\n");
  });
});

describe("readWorkingTreeFile", () => {
  it("reads a file in the working tree, as it is on disk", async () => {
    const repo = await createHistoryRepo("read");
    writeFileSync(join(paths.get("read")!, "crlf.txt"), "\uFEFFone\r\ntwo\r\n");
    expect(await readWorkingTreeFile(repo, "a file.txt")).toMatchObject({ contents: "changed\n" });
    expect((await readWorkingTreeFile(repo, "crlf.txt")).contents).toBe("\uFEFFone\r\ntwo\r\n");
  });

  it("won't read outside the working tree, or in the git directory", async () => {
    const repo = await createHistoryRepo("confined");
    const path = paths.get("confined")!;
    symlinkSync(join(path, ".."), join(path, "up"));
    symlinkSync(join(path, "c.txt"), join(path, "inside"));
    writeFileSync(join(path, "..", "outside.txt"), "secret\n");
    const outside = ["../outside.txt", "up/outside.txt", ".git/config", "/etc/hosts"];
    const errors = await Promise.all(
      outside.map((name) => rejection(readWorkingTreeFile(repo, name))),
    );
    for (const error of errors) expect(error).toBeInstanceOf(OutsideRepositoryError);
    expect((await readWorkingTreeFile(repo, "inside")).contents).toBe("b\n");
  });

  it("won't read the git directory through a link or in another case", async () => {
    const repo = await createHistoryRepo("git-dir");
    symlinkSync(".git", join(paths.get("git-dir")!, "meta"));
    const errors = await Promise.all(
      ["meta/config", ".GIT/config"].map((name) => rejection(readWorkingTreeFile(repo, name))),
    );
    for (const error of errors) expect(error).toBeInstanceOf(OutsideRepositoryError);
  });

  it("says when a file is gone, and won't wait on a named pipe", async () => {
    const repo = await createHistoryRepo("not-files");
    execFileSync("mkfifo", [join(paths.get("not-files")!, "pipe")]);
    expect(await rejection(readWorkingTreeFile(repo, "gone.txt"))).toBeInstanceOf(
      WorkingTreeFileNotFoundError,
    );
    expect(await rejection(readWorkingTreeFile(repo, "pipe"))).toBeInstanceOf(
      OutsideRepositoryError,
    );
  });

  it("won't read a file that's too large, or isn't UTF-8", async () => {
    const repo = await createHistoryRepo("unreadable");
    const path = paths.get("unreadable")!;
    writeFileSync(join(path, "large.txt"), "x".repeat(MAX_BLOB_BYTES + 1));
    writeFileSync(join(path, "latin1.txt"), Buffer.from([0x63, 0x61, 0x66, 0xe9]));
    expect(await rejection(readWorkingTreeFile(repo, "large.txt"))).toBeInstanceOf(
      FileTooLargeError,
    );
    expect(await rejection(readWorkingTreeFile(repo, "latin1.txt"))).toBeInstanceOf(NotUtf8Error);
  });
});

describe("saveWorkingTreeFile", () => {
  it("saves edits over the file as it was read, keeping its permissions", async () => {
    const repo = await createHistoryRepo("save");
    const path = paths.get("save")!;
    chmodSync(join(path, "a file.txt"), 0o755);
    const { version } = await readWorkingTreeFile(repo, "a file.txt");

    const saved = await saveWorkingTreeFile(repo, "a file.txt", "edited\n", {
      version,
      overwrite: false,
    });
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("edited\n");
    expect(statSync(join(path, "a file.txt")).mode & 0o777).toBe(0o755);
    expect(saved).toBe((await readWorkingTreeFile(repo, "a file.txt")).version);
    // Saved again from there, as edits go on.
    await saveWorkingTreeFile(repo, "a file.txt", "again\n", { version: saved, overwrite: false });
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("again\n");
    expect(readdirSync(path).filter((name) => name.endsWith(".gitto"))).toEqual([]);
  });

  it("keeps CRLF line ends, and a byte order mark", async () => {
    const repo = await createHistoryRepo("save-crlf");
    const path = paths.get("save-crlf")!;
    writeFileSync(join(path, "crlf.txt"), "\uFEFFone\r\ntwo\r\n");
    const { contents, version } = await readWorkingTreeFile(repo, "crlf.txt");

    // Edited as git shows it with `core.autocrlf`: with LF line ends.
    const edited = contents.replaceAll("\r\n", "\n").replace("two", "2");
    await saveWorkingTreeFile(repo, "crlf.txt", edited, { version, overwrite: false });
    expect(readFileSync(join(path, "crlf.txt"), "utf8")).toBe("\uFEFFone\r\n2\r\n");
  });

  it("won't save over a file that changed on disk since, unless told to", async () => {
    const repo = await createHistoryRepo("save-changed");
    const path = paths.get("save-changed")!;
    const { version } = await readWorkingTreeFile(repo, "a file.txt");
    writeFileSync(join(path, "a file.txt"), "from an editor\n");

    expect(
      await rejection(
        saveWorkingTreeFile(repo, "a file.txt", "mine\n", { version, overwrite: false }),
      ),
    ).toBeInstanceOf(FileChangedOnDiskError);
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("from an editor\n");
    await saveWorkingTreeFile(repo, "a file.txt", "mine\n", { version, overwrite: true });
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("mine\n");
  });

  it("won't save through a link, outside the working tree, or over a file that isn't UTF-8", async () => {
    const repo = await createHistoryRepo("save-refused");
    const path = paths.get("save-refused")!;
    symlinkSync(join(path, "c.txt"), join(path, "link"));
    writeFileSync(join(path, "latin1.txt"), Buffer.from([0x63, 0x61, 0x66, 0xe9]));
    const save = (name: string) =>
      rejection(saveWorkingTreeFile(repo, name, "x\n", { version: "", overwrite: true }));

    expect(await save("link")).toBeInstanceOf(OutsideRepositoryError);
    expect(await save("../outside.txt")).toBeInstanceOf(OutsideRepositoryError);
    expect(await save(".git/config")).toBeInstanceOf(OutsideRepositoryError);
    expect(await save("latin1.txt")).toBeInstanceOf(NotUtf8Error);
    expect(readFileSync(join(path, "c.txt"), "utf8")).toBe("b\n");
  });

  it("takes a save over the version an earlier save of its own started from", async () => {
    const repo = await createHistoryRepo("save-chained");
    const path = paths.get("save-chained")!;
    const { version } = await readWorkingTreeFile(repo, "a file.txt");
    // Both sent before either's done, as the last edits are when the window closes.
    await Promise.all([
      saveWorkingTreeFile(repo, "a file.txt", "first\n", { version, overwrite: false }),
      saveWorkingTreeFile(repo, "a file.txt", "second\n", { version, overwrite: false }),
    ]);
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("second\n");
    // Not once something else wrote it since.
    writeFileSync(join(path, "a file.txt"), "from an editor\n");
    expect(
      await rejection(
        saveWorkingTreeFile(repo, "a file.txt", "third\n", { version, overwrite: false }),
      ),
    ).toBeInstanceOf(FileChangedOnDiskError);
  });

  it("makes a file that was deleted again when overwriting, and only then", async () => {
    const repo = await createHistoryRepo("save-deleted");
    const path = paths.get("save-deleted")!;
    const { version } = await readWorkingTreeFile(repo, "a file.txt");
    rmSync(join(path, "a file.txt"));

    const error = await rejection(
      saveWorkingTreeFile(repo, "a file.txt", "mine\n", { version, overwrite: false }),
    );
    expect(error).toBeInstanceOf(FileChangedOnDiskError);
    expect(error).toMatchObject({ message: expect.stringContaining("was deleted") });
    await saveWorkingTreeFile(repo, "a file.txt", "mine\n", { version, overwrite: true });
    expect(readFileSync(join(path, "a file.txt"), "utf8")).toBe("mine\n");
    // A new file's permissions, not anyone's to write.
    expect(statSync(join(path, "a file.txt")).mode & 0o777).toBe(0o666 & ~process.umask());
  });
});
