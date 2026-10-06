import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  ConflictChangedError,
  ConflictMarkersError,
  FileChangedOnDiskError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import { createMergeConflict, tryGit } from "../../test/conflicts";
import { git, rejection, repos } from "../../test/fixtures";
import { saveWorkingTreeFile } from "../diff/working-tree";
import { stage, stageAll } from "../staging/commands";
import { getStatus } from "../status/commands";
import { parseStatus, STATUS_ARGS } from "../status/parse";
import { getConflict, keepSide, markResolved } from "./commands";
import type { Conflict } from "./schema";

/** Each changed file, with how it changed in the index and the working tree. */
async function statusFiles(repo: Repo) {
  return parseStatus(await repo.read(STATUS_ARGS)).files.map((file) => [
    file.path,
    file.staged,
    file.unstaged,
  ]);
}

/** What the user saw of `conflict`, to resolve it by. */
function shown(conflict: Conflict) {
  return { sides: conflict, version: conflict.version };
}

/** The object `git` has for `spec`, e.g. `:2:f.txt` for ours of a conflicted file. */
function oid(path: string, spec: string): string {
  return git(path, "rev-parse", spec);
}

describe("a text conflict", () => {
  it.each([
    ["merge", "one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\nthree\n"],
    ["diff3", "one\n<<<<<<< HEAD\nours\n||||||| BASE\ntwo\n=======\ntheirs\n>>>>>>> side\nthree\n"],
    [
      "zdiff3",
      "one\n<<<<<<< HEAD\nours\n||||||| BASE\ntwo\n=======\ntheirs\n>>>>>>> side\nthree\n",
    ],
  ] as const)("is read with its markers, in the %s style", async (style, markers) => {
    const path = createMergeConflict(`text-${style}`, style);
    const repo = await repos.open(`text-${style}`);
    // The base's marker names the commit the branches forked from.
    const contents = markers.replace("BASE", git(path, "rev-parse", "--short", "HEAD~"));

    const conflict = await getConflict(repo, "f.txt");
    expect(conflict).toEqual({
      base: { mode: "100644", oid: oid(path, ":1:f.txt") },
      ours: { mode: "100644", oid: oid(path, ":2:f.txt") },
      theirs: { mode: "100644", oid: oid(path, ":3:f.txt") },
      text: { contents, version: expect.any(String) },
      version: conflict.text?.version,
      binary: false,
      unreadable: null,
    });
    expect((await getStatus(repo)).changes.markerFree).toEqual([]);
  });

  it("is marked resolved once its markers are gone, and not before", async () => {
    const path = createMergeConflict("resolve");
    const repo = await repos.open("resolve");
    const { text } = await getConflict(repo, "f.txt");

    await expect(markResolved(repo, "f.txt", text!.version)).rejects.toBeInstanceOf(
      ConflictMarkersError,
    );
    const version = await saveWorkingTreeFile(repo, "f.txt", "one\nours\ntheirs\nthree\n", {
      version: text!.version,
      overwrite: false,
    });
    // Resolved in the working tree, but not marked so yet.
    expect((await getStatus(repo)).changes.markerFree).toEqual(["f.txt"]);
    expect(await statusFiles(repo)).toEqual([["f.txt", "conflicted", "conflicted"]]);

    await markResolved(repo, "f.txt", version);
    expect(await statusFiles(repo)).toEqual([["f.txt", "modified", null]]);
    expect(git(path, "show", ":f.txt")).toBe("one\nours\ntheirs\nthree");
    // Marking it again, once it isn't conflicted, does nothing.
    await markResolved(repo, "f.txt", version);
  });

  it("isn't marked resolved once it changed on disk since it was read", async () => {
    const path = createMergeConflict("resolve-changed");
    const repo = await repos.open("resolve-changed");
    const { text } = await getConflict(repo, "f.txt");
    writeFileSync(join(path, "f.txt"), "one\nchanged elsewhere\nthree\n");

    await expect(markResolved(repo, "f.txt", text!.version)).rejects.toBeInstanceOf(
      FileChangedOnDiskError,
    );
    expect(await statusFiles(repo)).toEqual([["f.txt", "conflicted", "conflicted"]]);
  });

  it("isn't staged with its markers, whole or with everything else", async () => {
    const path = createMergeConflict("refuse");
    const repo = await repos.open("refuse");
    writeFileSync(join(path, "README"), "changed\n");

    const error = await rejection(stage(repo, ["f.txt", "README"]));
    expect(error).toBeInstanceOf(ConflictMarkersError);
    expect((error as Error).message).toBe(
      "f.txt still has conflict markers. Resolve its conflicts, then mark it resolved.",
    );
    await expect(stageAll(repo)).rejects.toBeInstanceOf(ConflictMarkersError);
    // None was staged.
    expect(await statusFiles(repo)).toEqual([
      ["README", null, "modified"],
      ["f.txt", "conflicted", "conflicted"],
    ]);

    // Another file is, while the conflict is left.
    await stage(repo, ["README"]);
    expect(await statusFiles(repo)).toEqual([
      ["README", "modified", null],
      ["f.txt", "conflicted", "conflicted"],
    ]);
  });

  it("is staged with the rest once its markers are gone", async () => {
    const path = createMergeConflict("stage-all");
    const repo = await repos.open("stage-all");
    writeFileSync(join(path, "f.txt"), "one\nresolved\nthree\n");

    await stageAll(repo);
    expect(await statusFiles(repo)).toEqual([["f.txt", "modified", null]]);
  });

  it("keeps a side whole", async () => {
    const path = createMergeConflict("keep-theirs");
    const repo = await repos.open("keep-theirs");
    const conflict = await getConflict(repo, "f.txt");

    await keepSide(repo, "f.txt", "theirs", shown(conflict));
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\ntheirs\nthree\n");
    expect(await statusFiles(repo)).toEqual([["f.txt", "modified", null]]);
  });

  it("doesn't keep a side over a file that changed on disk, or a conflict that did", async () => {
    const path = createMergeConflict("keep-changed");
    const repo = await repos.open("keep-changed");
    const conflict = await getConflict(repo, "f.txt");

    writeFileSync(join(path, "f.txt"), "one\nedited\nthree\n");
    await expect(keepSide(repo, "f.txt", "ours", shown(conflict))).rejects.toBeInstanceOf(
      FileChangedOnDiskError,
    );
    git(path, "add", "f.txt");
    await expect(
      keepSide(repo, "f.txt", "ours", { sides: conflict, version: null }),
    ).rejects.toBeInstanceOf(ConflictChangedError);
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\nedited\nthree\n");
  });
});

describe("a file deleted on one side", () => {
  const sides = {
    base: { "f.txt": "one\n" },
    ours: { "f.txt": "one\nours\n" },
    theirs: { "f.txt": null },
  };

  it("is read as deleted in theirs", async () => {
    const path = createMergeConflict("deleted-by-them", undefined, sides);
    const conflict = await getConflict(await repos.open("deleted-by-them"), "f.txt");
    expect(conflict).toMatchObject({
      base: { oid: oid(path, ":1:f.txt") },
      ours: { oid: oid(path, ":2:f.txt") },
      theirs: null,
      // Git leaves ours in the working tree, without markers.
      text: { contents: "one\nours\n" },
    });
  });

  it("is deleted by keeping theirs, or kept by keeping ours", async () => {
    const path = createMergeConflict("delete-kept", undefined, sides);
    const repo = await repos.open("delete-kept");
    await keepSide(repo, "f.txt", "theirs", shown(await getConflict(repo, "f.txt")));
    expect(existsSync(join(path, "f.txt"))).toBe(false);
    expect(await statusFiles(repo)).toEqual([["f.txt", "deleted", null]]);

    createMergeConflict("modify-kept", undefined, sides);
    const other = await repos.open("modify-kept");
    await keepSide(other, "f.txt", "ours", shown(await getConflict(other, "f.txt")));
    expect(await statusFiles(other)).toEqual([]);
  });

  it("is read as deleted in ours, and brought back by keeping theirs", async () => {
    const path = createMergeConflict("deleted-by-us", undefined, {
      base: { "f.txt": "one\n" },
      ours: { "f.txt": null },
      theirs: { "f.txt": "one\ntheirs\n" },
    });
    const repo = await repos.open("deleted-by-us");
    const conflict = await getConflict(repo, "f.txt");
    expect(conflict).toMatchObject({ ours: null, theirs: { oid: oid(path, ":3:f.txt") } });

    await keepSide(repo, "f.txt", "theirs", shown(conflict));
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\ntheirs\n");
    expect(await statusFiles(repo)).toEqual([["f.txt", "added", null]]);
  });
});

describe("a file added on both sides", () => {
  it("has no base, and markers for the whole file", async () => {
    createMergeConflict("added-by-both", undefined, {
      base: {},
      ours: { "new.txt": "ours\n" },
      theirs: { "new.txt": "theirs\n" },
    });
    const conflict = await getConflict(await repos.open("added-by-both"), "new.txt");
    expect(conflict).toMatchObject({
      base: null,
      ours: { mode: "100644" },
      theirs: { mode: "100644" },
      text: { contents: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n" },
    });
  });
});

describe("a binary file", () => {
  const sides = {
    base: { "b.bin": Buffer.from([0, 1, 2]) },
    ours: { "b.bin": Buffer.from([0, 1, 3]) },
    theirs: { "b.bin": Buffer.from([0, 1, 4]) },
  };

  it("isn't read as text, and has no markers", async () => {
    createMergeConflict("binary", undefined, sides);
    const repo = await repos.open("binary");
    expect(await getConflict(repo, "b.bin")).toMatchObject({
      text: null,
      version: expect.any(String),
      binary: true,
      unreadable: "This file is binary.",
    });
    // Nor is it ready to be marked resolved: git just left ours.
    expect((await getStatus(repo)).changes.markerFree).toEqual([]);
  });

  it("is resolved by keeping a side", async () => {
    const path = createMergeConflict("binary-kept", undefined, sides);
    const repo = await repos.open("binary-kept");
    await keepSide(repo, "b.bin", "theirs", shown(await getConflict(repo, "b.bin")));
    expect([...readFileSync(join(path, "b.bin"))]).toEqual([0, 1, 4]);
    expect(await statusFiles(repo)).toEqual([["b.bin", "modified", null]]);
  });
});

describe("a file that isn't conflicted", () => {
  it("has no sides", async () => {
    const path = createMergeConflict("not-conflicted");
    tryGit(path, "merge", "--abort");
    expect(await getConflict(await repos.open("not-conflicted"), "f.txt")).toMatchObject({
      base: null,
      ours: null,
      theirs: null,
      text: { contents: "one\nours\nthree\n" },
    });
  });
});
