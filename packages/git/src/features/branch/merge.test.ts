import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  GitError,
  MergeBlockedError,
  MergeStoppedError,
  RepositoryChangedError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import { commitFiles, createDivergedRepo, TEXT_CONFLICT, tryGit } from "../../test/conflicts";
import { cloneRepo, createRepo, git, rejection, repos } from "../../test/fixtures";
import { getConflict, keepSide, markResolved } from "../conflicts/commands";
import { saveWorkingTreeFile } from "../diff/working-tree";
import { abortOperation, continueOperation, getOperation } from "../operation/commands";
import { popStash } from "../stash/commands";
import { mergeBranch, parseRefusal, refusalMessage } from "./merge";

/** `side` changed `s.txt`, `main` changed `m.txt`: they merge without conflicts. */
const CLEAN = {
  base: { "f.txt": "one\n" },
  ours: { "m.txt": "main\n" },
  theirs: { "s.txt": "side\n" },
};

/** The repository's state that a merge that did nothing leaves as it was. */
function snapshot(path: string) {
  return {
    head: git(path, "rev-parse", "HEAD"),
    branch: git(path, "branch", "--show-current"),
    status: git(path, "status", "--porcelain"),
    merging: existsSync(join(path, ".git", "MERGE_HEAD")),
  };
}

/** Resolves `f.txt`'s text conflict as `contents`, and marks it resolved, as the resolver does. */
async function resolveText(repo: Repo, contents: string): Promise<void> {
  const { text } = await getConflict(repo, "f.txt");
  const version = await saveWorkingTreeFile(repo, "f.txt", contents, {
    version: text!.version,
    overwrite: false,
  });
  await markResolved(repo, "f.txt", version);
}

describe("merging a branch", () => {
  it("fast-forwards to a branch that's ahead", async () => {
    const path = createRepo("merge-ff");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    git(path, "checkout", "-qb", "side");
    commitFiles(path, { "f.txt": "two\n" }, "second");
    git(path, "checkout", "-q", "main");

    expect(await mergeBranch(await repos.open("merge-ff"), "refs/heads/side", "main")).toBe(
      "fast-forward",
    );
    expect(git(path, "rev-parse", "main")).toBe(git(path, "rev-parse", "side"));
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("two\n");
    expect(git(path, "status", "--porcelain")).toBe("");
  });

  it("makes a merge commit for a branch that diverged, worded as git words it", async () => {
    const path = createDivergedRepo("merge-commit", CLEAN);
    const main = git(path, "rev-parse", "main");

    expect(await mergeBranch(await repos.open("merge-commit"), "refs/heads/side", "main")).toBe(
      "merged",
    );
    expect(git(path, "log", "-1", "--format=%B")).toBe("Merge branch 'side'");
    expect(git(path, "log", "-1", "--format=%P").split(" ")).toEqual([
      main,
      git(path, "rev-parse", "side"),
    ]);
    expect(readFileSync(join(path, "s.txt"), "utf8")).toBe("side\n");
    expect(git(path, "status", "--porcelain")).toBe("");
  });

  it("says which branch it merged into, when it isn't the main one", async () => {
    const path = createDivergedRepo("merge-into", CLEAN);
    git(path, "checkout", "-qb", "topic");

    await mergeBranch(await repos.open("merge-into"), "refs/heads/side", "topic");
    expect(git(path, "log", "-1", "--format=%s")).toBe("Merge branch 'side' into topic");
    expect(git(path, "rev-parse", "main")).not.toBe(git(path, "rev-parse", "topic"));
  });

  it("merges a remote branch, as a remote-tracking one", async () => {
    const upstream = createDivergedRepo("merge-upstream", CLEAN);
    const path = cloneRepo("merge-remote", upstream);
    commitFiles(path, { "c.txt": "clone\n" }, "clone");

    expect(
      await mergeBranch(await repos.open("merge-remote"), "refs/remotes/origin/side", "main"),
    ).toBe("merged");
    expect(git(path, "log", "-1", "--format=%s")).toBe(
      "Merge remote-tracking branch 'origin/side'",
    );
    expect(git(path, "rev-parse", "HEAD^2")).toBe(git(path, "rev-parse", "origin/side"));
  });

  it("does nothing when the branch has every commit already", async () => {
    const path = createRepo("merge-up-to-date");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    git(path, "branch", "old");
    commitFiles(path, { "f.txt": "two\n" }, "second");
    const before = snapshot(path);

    expect(await mergeBranch(await repos.open("merge-up-to-date"), "refs/heads/old", "main")).toBe(
      "up-to-date",
    );
    expect(snapshot(path)).toEqual(before);
  });

  it("fast-forwards a branch without commits yet to the one merged", async () => {
    const path = createRepo("merge-unborn");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    git(path, "checkout", "-q", "--orphan", "empty");
    git(path, "rm", "-rqf", ".");

    expect(await mergeBranch(await repos.open("merge-unborn"), "refs/heads/main", "empty")).toBe(
      "fast-forward",
    );
    expect(git(path, "rev-parse", "empty")).toBe(git(path, "rev-parse", "main"));
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\n");
  });

  it("merges the branch, not a tag of the same name, labelling its conflicts by full name", async () => {
    const path = createDivergedRepo("merge-tag-name", TEXT_CONFLICT);
    git(path, "tag", "side", "main~");
    const repo = await repos.open("merge-tag-name");

    expect(await mergeBranch(repo, "refs/heads/side", "main")).toBe("conflicts");
    expect(readFileSync(join(path, "f.txt"), "utf8")).toContain(">>>>>>> refs/heads/side\n");
    await resolveText(repo, "one\nboth\nthree\n");
    await continueOperation(repo, "merge");
    expect(git(path, "rev-parse", "HEAD^2")).toBe(git(path, "rev-parse", "refs/heads/side"));
    expect(git(path, "log", "-1", "--format=%s")).toBe("Merge branch 'side'");
  });

  it("keeps the uncommitted changes the merge doesn't touch", async () => {
    const path = createDivergedRepo("merge-keeps-changes", CLEAN);
    writeFileSync(join(path, "f.txt"), "changed\n");
    writeFileSync(join(path, "new.txt"), "new\n");

    expect(
      await mergeBranch(await repos.open("merge-keeps-changes"), "refs/heads/side", "main"),
    ).toBe("merged");
    expect(git(path, "status", "--porcelain")).toBe("M f.txt\n?? new.txt");
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("changed\n");
  });

  describe("with the user's settings", () => {
    it("makes a merge commit even for a fast-forward, with merge.ff false", async () => {
      const path = createRepo("merge-no-ff");
      commitFiles(path, { "f.txt": "one\n" }, "first");
      git(path, "checkout", "-qb", "side");
      commitFiles(path, { "f.txt": "two\n" }, "second");
      git(path, "checkout", "-q", "main");
      git(path, "config", "merge.ff", "false");

      expect(await mergeBranch(await repos.open("merge-no-ff"), "refs/heads/side", "main")).toBe(
        "merged",
      );
      expect(git(path, "log", "-1", "--format=%s")).toBe("Merge branch 'side'");
    });

    it("refuses a branch that diverged, with merge.ff only, doing nothing", async () => {
      const path = createDivergedRepo("merge-ff-only", CLEAN);
      git(path, "config", "merge.ff", "only");
      const before = snapshot(path);

      const error = await rejection(
        mergeBranch(await repos.open("merge-ff-only"), "refs/heads/side", "main"),
      );
      expect(error).toBeInstanceOf(GitError);
      expect((error as Error).message).toBe("fatal: Not possible to fast-forward, aborting.");
      expect(snapshot(path)).toEqual(before);
    });

    it("adds the log of what's merged to the message, with merge.log", async () => {
      const path = createDivergedRepo("merge-log", CLEAN);
      git(path, "config", "merge.log", "true");

      await mergeBranch(await repos.open("merge-log"), "refs/heads/side", "main");
      expect(git(path, "log", "-1", "--format=%B")).toBe(
        "Merge branch 'side'\n\n* side:\n  theirs",
      );
    });

    it("commits the merge, unsquashed, whatever the branch's merge options say", async () => {
      const path = createDivergedRepo("merge-options", CLEAN);
      git(path, "config", "branch.main.mergeOptions", "--squash --no-commit");

      expect(await mergeBranch(await repos.open("merge-options"), "refs/heads/side", "main")).toBe(
        "merged",
      );
      expect(git(path, "rev-parse", "HEAD^2")).toBe(git(path, "rev-parse", "side"));
      expect(await getOperation(await repos.open("merge-options"))).toBeNull();
    });

    it("doesn't stash the changes it would overwrite, whatever merge.autoStash says", async () => {
      const path = createDivergedRepo("merge-autostash", CLEAN);
      git(path, "config", "merge.autoStash", "true");
      git(path, "checkout", "-q", "side");
      commitFiles(path, { "m.txt": "side's\n" }, "side's m.txt");
      git(path, "checkout", "-q", "main");
      writeFileSync(join(path, "m.txt"), "mine\n");

      await rejection(mergeBranch(await repos.open("merge-autostash"), "refs/heads/side", "main"));
      expect(git(path, "stash", "list")).toBe("");
      expect(readFileSync(join(path, "m.txt"), "utf8")).toBe("mine\n");
    });
  });

  describe("at conflicts", () => {
    it("stops, leaving the merge under way to resolve", async () => {
      const path = createDivergedRepo("merge-conflicts", TEXT_CONFLICT);
      const repo = await repos.open("merge-conflicts");

      expect(await mergeBranch(repo, "refs/heads/side", "main")).toBe("conflicts");
      expect(await getOperation(repo)).toEqual({ kind: "merge", merging: "side", into: "main" });
      expect(readFileSync(join(path, "f.txt"), "utf8")).toBe(
        "one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\nthree\n",
      );
      expect(git(path, "status", "--porcelain")).toBe("UU f.txt");
    });

    it("commits the merge with its message once they're resolved", async () => {
      const path = createDivergedRepo("merge-resolved", TEXT_CONFLICT);
      const repo = await repos.open("merge-resolved");
      const main = git(path, "rev-parse", "main");

      await mergeBranch(repo, "refs/heads/side", "main");
      await resolveText(repo, "one\nboth\nthree\n");
      await continueOperation(repo, "merge");

      expect(await getOperation(repo)).toBeNull();
      expect(git(path, "log", "-1", "--format=%B")).toBe("Merge branch 'side'");
      expect(git(path, "log", "-1", "--format=%P").split(" ")).toEqual([
        main,
        git(path, "rev-parse", "side"),
      ]);
      expect(git(path, "show", "HEAD:f.txt")).toBe("one\nboth\nthree");
    });

    it("commits the message of a merge into another branch, and of a remote one", async () => {
      const upstream = createDivergedRepo("merge-resolved-upstream", TEXT_CONFLICT);
      const path = cloneRepo("merge-resolved-remote", upstream);
      git(path, "checkout", "-qb", "topic");
      const repo = await repos.open("merge-resolved-remote");

      expect(await mergeBranch(repo, "refs/remotes/origin/side", "topic")).toBe("conflicts");
      expect(readFileSync(join(path, "f.txt"), "utf8")).toContain(">>>>>>> origin/side\n");
      expect(await getOperation(repo)).toEqual({
        kind: "merge",
        merging: "origin/side",
        into: "topic",
      });
      await resolveText(repo, "one\nboth\nthree\n");
      await continueOperation(repo, "merge");
      expect(git(path, "log", "-1", "--format=%s")).toBe(
        "Merge remote-tracking branch 'origin/side' into topic",
      );
    });

    it("is aborted, putting everything back as it was", async () => {
      const path = createDivergedRepo("merge-abort", TEXT_CONFLICT);
      const repo = await repos.open("merge-abort");
      const before = snapshot(path);

      await mergeBranch(repo, "refs/heads/side", "main");
      await abortOperation(repo, "merge");
      expect(snapshot(path)).toEqual(before);
      expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\nours\nthree\n");
    });

    it("stops at a file deleted on one side, resolved by keeping a side whole", async () => {
      const path = createDivergedRepo("merge-deleted", {
        base: { "f.txt": "one\n" },
        ours: { "f.txt": null },
        theirs: { "f.txt": "changed\n" },
      });
      const repo = await repos.open("merge-deleted");

      expect(await mergeBranch(repo, "refs/heads/side", "main")).toBe("conflicts");
      const { base, ours, theirs } = await getConflict(repo, "f.txt");
      expect(ours).toBeNull();
      await keepSide(repo, "f.txt", "theirs", { sides: { base, ours, theirs }, version: null });
      await continueOperation(repo, "merge");
      expect(git(path, "show", "HEAD:f.txt")).toBe("changed");
      expect(git(path, "log", "-1", "--format=%s")).toBe("Merge branch 'side'");
    });

    it("stops at a binary file changed on both sides", async () => {
      createDivergedRepo("merge-binary", {
        base: { "b.dat": Buffer.from([0, 1]) },
        ours: { "b.dat": Buffer.from([0, 2]) },
        theirs: { "b.dat": Buffer.from([0, 3]) },
      });
      const repo = await repos.open("merge-binary");

      expect(await mergeBranch(repo, "refs/heads/side", "main")).toBe("conflicts");
      const conflict = await getConflict(repo, "b.dat");
      expect(conflict.ours).not.toBeNull();
      expect(conflict.theirs).not.toBeNull();
      expect(conflict.text).toBeNull();
    });

    it("keeps the uncommitted changes to other files", async () => {
      const path = createDivergedRepo("merge-conflicts-changes", {
        ...TEXT_CONFLICT,
        base: { ...TEXT_CONFLICT.base, "g.txt": "g\n" },
      });
      writeFileSync(join(path, "g.txt"), "changed\n");

      expect(
        await mergeBranch(await repos.open("merge-conflicts-changes"), "refs/heads/side", "main"),
      ).toBe("conflicts");
      expect(git(path, "status", "--porcelain")).toBe("UU f.txt\n M g.txt");
    });
  });

  describe("refuses, doing nothing,", () => {
    it("over uncommitted changes to a file it changes", async () => {
      const path = createDivergedRepo("merge-over-changes", TEXT_CONFLICT);
      writeFileSync(join(path, "f.txt"), "mine\n");
      const before = snapshot(path);

      await expect(
        mergeBranch(await repos.open("merge-over-changes"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Merging side would overwrite your uncommitted changes to f.txt. Commit or stash them, then merge.",
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("over uncommitted changes to several files, naming the first", async () => {
      const path = createDivergedRepo("merge-over-several", {
        base: { "a.txt": "a\n", "b.txt": "b\n", "c.txt": "c\n" },
        ours: { "m.txt": "main\n" },
        theirs: { "a.txt": "A\n", "b.txt": "B\n", "c.txt": "C\n" },
      });
      for (const file of ["a.txt", "b.txt", "c.txt"]) writeFileSync(join(path, file), "mine\n");

      await expect(
        mergeBranch(await repos.open("merge-over-several"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Merging side would overwrite your uncommitted changes to a.txt and 2 other files. Commit or stash them, then merge.",
      );
    });

    it("over staged changes, which git won't merge with, even to files it doesn't change", async () => {
      const path = createDivergedRepo("merge-over-staged", CLEAN);
      writeFileSync(join(path, "f.txt"), "staged\n");
      git(path, "add", "f.txt");
      const before = snapshot(path);

      await expect(
        mergeBranch(await repos.open("merge-over-staged"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Git won't merge side while changes are staged, to f.txt. Commit or stash them, then merge.",
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("over staged changes to several files, a name with a space among them", async () => {
      const path = createDivergedRepo("merge-over-staged-several", {
        ...CLEAN,
        base: { "a b.txt": "a\n", "f.txt": "f\n" },
      });
      writeFileSync(join(path, "a b.txt"), "staged\n");
      writeFileSync(join(path, "f.txt"), "staged\n");
      git(path, "add", "-A");

      await expect(
        mergeBranch(await repos.open("merge-over-staged-several"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Git won't merge side while changes are staged, to a b.txt and f.txt. Commit or stash them, then merge.",
      );
    });

    it("over untracked files in a folder it replaces", async () => {
      // `d` is a folder on `main`, which `side` makes a file.
      const path = createRepo("merge-over-folder");
      mkdirSync(join(path, "d"));
      commitFiles(path, { "d/k.txt": "k\n" }, "base");
      git(path, "checkout", "-qb", "side");
      git(path, "rm", "-rq", "d");
      commitFiles(path, { d: "now a file\n" }, "d is a file");
      git(path, "checkout", "-q", "main");
      commitFiles(path, { "m.txt": "main\n" }, "ours");
      writeFileSync(join(path, "d", "untracked.txt"), "mine\n");
      const before = snapshot(path);

      await expect(
        mergeBranch(await repos.open("merge-over-folder"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Merging side would lose the untracked files in the folder d. Move or remove them, then merge.",
      );
      expect(snapshot(path)).toEqual(before);
      expect(readFileSync(join(path, "d", "untracked.txt"), "utf8")).toBe("mine\n");
    });

    it("over an untracked file in the way of one it adds", async () => {
      const path = createDivergedRepo("merge-over-untracked", CLEAN);
      writeFileSync(join(path, "s.txt"), "mine\n");
      const before = snapshot(path);

      await expect(
        mergeBranch(await repos.open("merge-over-untracked"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Merging side would overwrite the untracked file s.txt. Move or remove it, then merge.",
      );
      expect(snapshot(path)).toEqual(before);
      expect(readFileSync(join(path, "s.txt"), "utf8")).toBe("mine\n");
    });

    it("over an untracked file in the way of a fast-forward", async () => {
      const path = createRepo("merge-ff-untracked");
      commitFiles(path, { "f.txt": "one\n" }, "first");
      git(path, "checkout", "-qb", "side");
      commitFiles(path, { "s.txt": "side\n", "t.txt": "side\n" }, "second");
      git(path, "checkout", "-q", "main");
      writeFileSync(join(path, "s.txt"), "mine\n");
      writeFileSync(join(path, "t.txt"), "mine\n");

      await expect(
        mergeBranch(await repos.open("merge-ff-untracked"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        "Merging side would overwrite the untracked files s.txt and t.txt. Move or remove them, then merge.",
      );
      expect(git(path, "rev-parse", "main")).not.toBe(git(path, "rev-parse", "side"));
    });

    it("while a merge is under way", async () => {
      const path = createDivergedRepo("merge-during-merge", TEXT_CONFLICT);
      git(path, "branch", "other", "main~");
      const repo = await repos.open("merge-during-merge");
      await mergeBranch(repo, "refs/heads/side", "main");
      // Resolved, but not committed yet: still under way.
      await resolveText(repo, "one\nboth\nthree\n");

      await expect(mergeBranch(repo, "refs/heads/other", "main")).rejects.toThrow(
        new MergeBlockedError("A merge is under way. Finish or abort it, then merge."),
      );
      expect(await getOperation(repo)).toEqual({ kind: "merge", merging: "side", into: "main" });
    });

    it("while a rebase is under way", async () => {
      const path = createDivergedRepo("merge-during-rebase", TEXT_CONFLICT);
      git(path, "branch", "other", "main");
      git(path, "checkout", "-q", "side");
      tryGit(path, "rebase", "main");

      await expect(
        mergeBranch(await repos.open("merge-during-rebase"), "refs/heads/other", "side"),
      ).rejects.toThrow(
        new MergeBlockedError("A rebase is under way. Finish or abort it, then merge."),
      );
    });

    it("while a cherry-pick is under way", async () => {
      const path = createDivergedRepo("merge-during-pick", TEXT_CONFLICT);
      tryGit(path, "cherry-pick", "side");

      await expect(
        mergeBranch(await repos.open("merge-during-pick"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        new MergeBlockedError("A cherry-pick is under way. Finish or abort it, then merge."),
      );
    });

    it("while a popped stash's conflicts are left to resolve", async () => {
      const path = createDivergedRepo("merge-during-pop", CLEAN);
      writeFileSync(join(path, "f.txt"), "stashed\n");
      git(path, "stash", "push", "-q");
      commitFiles(path, { "f.txt": "committed\n" }, "second");
      const repo = await repos.open("merge-during-pop");
      await popStash(repo, git(path, "rev-parse", "refs/stash")).catch(() => undefined);
      const before = snapshot(path);

      await expect(mergeBranch(repo, "refs/heads/side", "main")).rejects.toThrow(
        new MergeBlockedError("Some files have conflicts. Resolve them, then merge."),
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("with HEAD detached", async () => {
      const path = createDivergedRepo("merge-detached", CLEAN);
      git(path, "checkout", "-q", "--detach");
      const before = snapshot(path);

      await expect(
        mergeBranch(await repos.open("merge-detached"), "refs/heads/side", "main"),
      ).rejects.toThrow(
        new MergeBlockedError("HEAD is detached: check out a branch to merge into it."),
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("once another branch is checked out than the one the user saw", async () => {
      const path = createDivergedRepo("merge-switched", CLEAN);
      git(path, "checkout", "-qb", "topic");
      const before = snapshot(path);

      const error = await rejection(
        mergeBranch(await repos.open("merge-switched"), "refs/heads/side", "main"),
      );
      expect(error).toBeInstanceOf(RepositoryChangedError);
      expect((error as Error).message).toBe(
        "Switched from main to topic meanwhile, so nothing was merged.",
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("once the branch is gone", async () => {
      const path = createDivergedRepo("merge-gone", CLEAN);
      git(path, "branch", "-D", "side");

      await expect(
        mergeBranch(await repos.open("merge-gone"), "refs/heads/side", "main"),
      ).rejects.toThrow(new MergeBlockedError("side no longer exists."));
    });

    it("for the branch checked out", async () => {
      createDivergedRepo("merge-itself", CLEAN);

      await expect(
        mergeBranch(await repos.open("merge-itself"), "refs/heads/main", "main"),
      ).rejects.toThrow(new MergeBlockedError("Can't merge main into itself."));
    });

    it("for a revision, rather than a branch", async () => {
      createDivergedRepo("merge-revision", CLEAN);

      await expect(
        mergeBranch(await repos.open("merge-revision"), "refs/heads/side~1", "main"),
      ).rejects.toThrow(new MergeBlockedError("side~1 no longer exists."));
    });

    it("for a ref that isn't a branch", async () => {
      const path = createDivergedRepo("merge-not-branch", CLEAN);
      git(path, "tag", "v1", "side");

      await expect(
        mergeBranch(await repos.open("merge-not-branch"), "refs/tags/v1", "main"),
      ).rejects.toThrow(new MergeBlockedError("refs/tags/v1 isn't a branch."));
    });
  });

  it("stops before committing when the merge commit can't be signed, to commit later", async () => {
    const path = createDivergedRepo("merge-unsigned", CLEAN);
    const gpg = join(path, ".git", "no-gpg");
    writeFileSync(gpg, "#!/bin/sh\nexit 1\n");
    chmodSync(gpg, 0o755);
    git(path, "config", "commit.gpgSign", "true");
    git(path, "config", "gpg.program", gpg);
    const repo = await repos.open("merge-unsigned");

    const error = await rejection(mergeBranch(repo, "refs/heads/side", "main"));
    expect(error).toBeInstanceOf(MergeStoppedError);
    expect((error as Error).message).toMatch(
      /^Merging side stopped before committing:\n[^]*failed to write commit object\nFix that, then commit the merge, or abort it\.$/,
    );
    // Left under way, rather than its result staged with nothing to say so.
    expect(await getOperation(repo)).toEqual({ kind: "merge", merging: "side", into: "main" });

    git(path, "config", "commit.gpgSign", "false");
    await continueOperation(repo, "merge");
    expect(git(path, "rev-parse", "HEAD^2")).toBe(git(path, "rev-parse", "side"));
  });

  it("stops before committing when a hook turns the merge commit down, to commit later", async () => {
    const path = createDivergedRepo("merge-hook", CLEAN);
    const hook = join(path, ".git", "hooks", "pre-merge-commit");
    writeFileSync(hook, "#!/bin/sh\necho 'Not on Fridays.' >&2\nexit 1\n");
    chmodSync(hook, 0o755);
    const repo = await repos.open("merge-hook");

    const error = await rejection(mergeBranch(repo, "refs/heads/side", "main"));
    expect(error).toBeInstanceOf(MergeStoppedError);
    expect((error as Error).message).toBe(
      "Merging side stopped before committing:\nNot on Fridays.\nNot committing merge; use 'git commit' to complete the merge.\nFix that, then commit the merge, or abort it.",
    );
    expect(await getOperation(repo)).toEqual({ kind: "merge", merging: "side", into: "main" });

    writeFileSync(hook, "#!/bin/sh\nexit 0\n");
    await continueOperation(repo, "merge");
    expect(git(path, "log", "-1", "--format=%s")).toBe("Merge branch 'side'");
    expect(git(path, "rev-parse", "HEAD^2")).toBe(git(path, "rev-parse", "side"));
  });
});

describe("what git says when it refuses to merge", () => {
  it("is read for each reason, with the files it lists", () => {
    expect(
      parseRefusal(
        "error: The following untracked working tree files would be removed by merge:\n\tgone.txt\n\t a b \nPlease move or remove them before you merge.\nAborting\n",
      ),
    ).toEqual({ kind: "removed", files: ["gone.txt", " a b "] });
    expect(
      parseRefusal(
        "error: The following untracked working tree files would be overwritten by merge:\n\tnew.txt\nPlease move or remove them before you merge.\n",
      ),
    ).toEqual({ kind: "overwritten", files: ["new.txt"] });
    expect(
      parseRefusal(
        "error: Your local changes to the following files would be overwritten by merge:\n\ta.txt\n\tb.txt\nPlease commit your changes or stash them before you merge.\n",
      ),
    ).toEqual({ kind: "changes", files: ["a.txt", "b.txt"] });
    expect(
      parseRefusal(
        "error: Your local changes to the following files would be overwritten by merge:\n  a.txt b c.txt\nMerge with strategy ort failed.\n",
      ),
    ).toEqual({ kind: "staged", files: [] });
    expect(
      parseRefusal(
        "error: Updating the following directories would lose untracked files in them:\n\td\n\nAborting\n",
      ),
    ).toEqual({ kind: "folders", files: ["d"] });
    expect(parseRefusal("fatal: Not possible to fast-forward, aborting.\n")).toBeUndefined();
  });

  it("is put to the user", () => {
    expect(refusalMessage("removed", "side", ["gone.txt"])).toBe(
      "Merging side would remove the untracked file gone.txt. Move or remove it, then merge.",
    );
    expect(refusalMessage("removed", "side", ["a", "b", "c"])).toBe(
      "Merging side would remove the untracked files a and 2 other files. Move or remove them, then merge.",
    );
    expect(refusalMessage("folders", "side", ["d", "e"])).toBe(
      "Merging side would lose the untracked files in the folders d and e. Move or remove them, then merge.",
    );
  });
});
