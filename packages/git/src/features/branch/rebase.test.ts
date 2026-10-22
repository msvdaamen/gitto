import { chmodSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import {
  GitError,
  RebaseBlockedError,
  RebaseStoppedError,
  RebaseUndoneError,
  RepositoryChangedError,
} from "../../core/errors";
import type { Repo } from "../../core/repo";
import { commitFiles, createDivergedRepo, TEXT_CONFLICT, tryGit } from "../../test/conflicts";
import { cloneRepo, createRepo, git, rejection, repos } from "../../test/fixtures";
import { getConflict, keepSide, markResolved } from "../conflicts/commands";
import { saveWorkingTreeFile } from "../diff/working-tree";
import { abortOperation, continueOperation, getOperation } from "../operation/commands";
import { popStash } from "../stash/commands";
import { parseUncommitted, rebaseBranch } from "./rebase";

/** `side` changed `s.txt`, `main` changed `m.txt`: `side` rebases onto `main` without conflicts. */
const CLEAN = {
  base: { "f.txt": "one\n" },
  ours: { "m.txt": "main\n" },
  theirs: { "s.txt": "side\n" },
};

/**
 * A repository like `createDivergedRepo` makes, with `side` checked out, to rebase onto `main`,
 * and a second commit on `side` if `more` is given, changing `f.txt` again.
 */
function createSideRepo(
  name: string,
  sides: {
    base: Record<string, string>;
    ours: Record<string, string>;
    theirs: Record<string, string>;
  },
  more?: string,
): string {
  const path = createDivergedRepo(name, sides);
  git(path, "checkout", "-q", "side");
  if (more !== undefined) commitFiles(path, { "f.txt": more }, "theirs again");
  return path;
}

/** The repository's state that a rebase that did nothing leaves as it was. */
function snapshot(path: string) {
  return {
    head: git(path, "rev-parse", "HEAD"),
    branch: git(path, "branch", "--show-current"),
    status: git(path, "status", "--porcelain"),
    rebasing: existsSync(join(path, ".git", "rebase-merge")),
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

describe("rebasing a branch", () => {
  it("replays the branch's commits on top of the other's", async () => {
    const path = createSideRepo("rebase-replay", CLEAN);
    commitFiles(path, { "t.txt": "side too\n" }, "theirs too");
    const main = git(path, "rev-parse", "main");

    expect(await rebaseBranch(await repos.open("rebase-replay"), "refs/heads/main", "side")).toBe(
      "rebased",
    );
    expect(git(path, "log", "--format=%s", "main..side")).toBe("theirs too\ntheirs");
    expect(git(path, "rev-parse", "side~2")).toBe(main);
    expect(git(path, "branch", "--show-current")).toBe("side");
    expect(readFileSync(join(path, "m.txt"), "utf8")).toBe("main\n");
    expect(readFileSync(join(path, "s.txt"), "utf8")).toBe("side\n");
    expect(git(path, "status", "--porcelain")).toBe("");
    expect(await getOperation(await repos.open("rebase-replay"))).toBeNull();
  });

  it("moves a branch without commits of its own to the other", async () => {
    const path = createRepo("rebase-ff");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    git(path, "checkout", "-qb", "side");
    git(path, "checkout", "-q", "main");
    commitFiles(path, { "f.txt": "two\n" }, "second");
    git(path, "checkout", "-q", "side");

    expect(await rebaseBranch(await repos.open("rebase-ff"), "refs/heads/main", "side")).toBe(
      "fast-forward",
    );
    expect(git(path, "rev-parse", "side")).toBe(git(path, "rev-parse", "main"));
    expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("two\n");
  });

  it("rebases onto a remote branch", async () => {
    const upstream = createDivergedRepo("rebase-upstream", CLEAN);
    const path = cloneRepo("rebase-remote", upstream);
    commitFiles(path, { "c.txt": "clone\n" }, "clone");

    expect(
      await rebaseBranch(await repos.open("rebase-remote"), "refs/remotes/origin/side", "main"),
    ).toBe("rebased");
    expect(git(path, "log", "--format=%s", "origin/side..main")).toBe("clone\nours");
    expect(git(path, "rev-parse", "main~2")).toBe(git(path, "rev-parse", "origin/side"));
  });

  it("does nothing when the branch is on top of the other already", async () => {
    const path = createRepo("rebase-up-to-date");
    commitFiles(path, { "f.txt": "one\n" }, "first");
    git(path, "branch", "old");
    commitFiles(path, { "f.txt": "two\n" }, "second");
    const before = snapshot(path);

    expect(
      await rebaseBranch(await repos.open("rebase-up-to-date"), "refs/heads/old", "main"),
    ).toBe("up-to-date");
    expect(snapshot(path)).toEqual(before);
  });

  it("rebases onto the branch, not a tag of the same name", async () => {
    const path = createSideRepo("rebase-tag-name", CLEAN);
    git(path, "tag", "main", "main~");

    expect(await rebaseBranch(await repos.open("rebase-tag-name"), "refs/heads/main", "side")).toBe(
      "rebased",
    );
    expect(git(path, "rev-parse", "side~")).toBe(git(path, "rev-parse", "refs/heads/main"));
  });

  it("keeps the untracked files that aren't in the way", async () => {
    const path = createSideRepo("rebase-keeps-untracked", CLEAN);
    writeFileSync(join(path, "new.txt"), "new\n");

    expect(
      await rebaseBranch(await repos.open("rebase-keeps-untracked"), "refs/heads/main", "side"),
    ).toBe("rebased");
    expect(git(path, "status", "--porcelain")).toBe("?? new.txt");
  });

  it("leaves fixup commits as they are, whatever rebase.autoSquash says", async () => {
    const path = createSideRepo("rebase-autosquash", CLEAN);
    commitFiles(path, { "s.txt": "side again\n" }, "fixup! theirs");
    git(path, "config", "rebase.autoSquash", "true");

    expect(
      await rebaseBranch(await repos.open("rebase-autosquash"), "refs/heads/main", "side"),
    ).toBe("rebased");
    expect(git(path, "log", "--format=%s", "main..side")).toBe("fixup! theirs\ntheirs");
  });

  describe("at conflicts", () => {
    it("stops, leaving the rebase under way to resolve", async () => {
      const path = createSideRepo("rebase-conflicts", TEXT_CONFLICT);
      const repo = await repos.open("rebase-conflicts");

      expect(await rebaseBranch(repo, "refs/heads/main", "side")).toBe("conflicts");
      expect(await getOperation(repo)).toEqual({
        kind: "rebase",
        branch: "side",
        onto: "main",
        steps: { step: 1, total: 1 },
      });
      expect(readFileSync(join(path, "f.txt"), "utf8")).toMatch(
        /^one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> [0-9a-f]+ \(theirs\)\nthree\n$/,
      );
      expect(git(path, "status", "--porcelain")).toBe("UU f.txt");
    });

    it("goes on with the next commit once they're resolved, and stops at its conflicts too", async () => {
      const path = createSideRepo("rebase-resolved", TEXT_CONFLICT, "one\ntheirs again\nthree\n");
      const repo = await repos.open("rebase-resolved");
      const main = git(path, "rev-parse", "main");

      expect(await rebaseBranch(repo, "refs/heads/main", "side")).toBe("conflicts");
      expect(await getOperation(repo)).toMatchObject({ steps: { step: 1, total: 2 } });
      await resolveText(repo, "one\nboth\nthree\n");
      await continueOperation(repo, "rebase");
      // The second commit conflicts with how the first was resolved.
      expect(await getOperation(repo)).toMatchObject({ steps: { step: 2, total: 2 } });
      expect(git(path, "status", "--porcelain")).toBe("UU f.txt");

      await resolveText(repo, "one\nboth again\nthree\n");
      await continueOperation(repo, "rebase");
      expect(await getOperation(repo)).toBeNull();
      expect(git(path, "log", "--format=%s", "main..side")).toBe("theirs again\ntheirs");
      expect(git(path, "rev-parse", "side~2")).toBe(main);
      expect(git(path, "show", "HEAD:f.txt")).toBe("one\nboth again\nthree");
      expect(git(path, "branch", "--show-current")).toBe("side");
    });

    it("stops at conflicts with a remote branch, named as the user knows it", async () => {
      const upstream = createDivergedRepo("rebase-conflicts-upstream", TEXT_CONFLICT);
      const path = cloneRepo("rebase-conflicts-remote", upstream);
      git(path, "checkout", "-q", "side");
      const repo = await repos.open("rebase-conflicts-remote");

      expect(await rebaseBranch(repo, "refs/remotes/origin/main", "side")).toBe("conflicts");
      expect(await getOperation(repo)).toEqual({
        kind: "rebase",
        branch: "side",
        onto: "main",
        steps: { step: 1, total: 1 },
      });
      await resolveText(repo, "one\nboth\nthree\n");
      await continueOperation(repo, "rebase");
      expect(git(path, "rev-parse", "side~")).toBe(git(path, "rev-parse", "origin/main"));
    });

    it("is aborted, putting everything back as it was", async () => {
      const path = createSideRepo("rebase-abort", TEXT_CONFLICT);
      const repo = await repos.open("rebase-abort");
      const before = snapshot(path);

      await rebaseBranch(repo, "refs/heads/main", "side");
      await abortOperation(repo, "rebase");
      expect(snapshot(path)).toEqual(before);
      expect(readFileSync(join(path, "f.txt"), "utf8")).toBe("one\ntheirs\nthree\n");
    });

    it("stops at a file deleted on one side, resolved by keeping a side whole", async () => {
      const path = createSideRepo("rebase-deleted", {
        base: { "f.txt": "one\n" },
        ours: { "f.txt": "changed\n" },
        theirs: { "f.txt": null as unknown as string },
      });
      const repo = await repos.open("rebase-deleted");

      expect(await rebaseBranch(repo, "refs/heads/main", "side")).toBe("conflicts");
      const { base, ours, theirs } = await getConflict(repo, "f.txt");
      // In a rebase, "ours" is the branch rebased onto, and "theirs" the commit replayed.
      expect(ours).not.toBeNull();
      expect(theirs).toBeNull();
      await keepSide(repo, "f.txt", "ours", { sides: { base, ours, theirs }, version: null });
      await continueOperation(repo, "rebase");
      expect(await getOperation(repo)).toBeNull();
      expect(git(path, "show", "HEAD:f.txt")).toBe("changed");
    });
  });

  describe("refuses, doing nothing,", () => {
    it("over uncommitted changes, naming the files", async () => {
      const path = createSideRepo("rebase-over-changes", CLEAN);
      writeFileSync(join(path, "f.txt"), "mine\n");
      const before = snapshot(path);

      await expect(
        rebaseBranch(await repos.open("rebase-over-changes"), "refs/heads/main", "side"),
      ).rejects.toThrow(
        new RebaseBlockedError(
          "Git won't rebase while there are uncommitted changes to f.txt. Commit or stash them, then rebase.",
        ),
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("over staged changes", async () => {
      const path = createSideRepo("rebase-over-staged", {
        ...CLEAN,
        base: { "a b.txt": "a\n", "f.txt": "f\n" },
      });
      writeFileSync(join(path, "a b.txt"), "staged\n");
      writeFileSync(join(path, "f.txt"), "staged\n");
      git(path, "add", "-A");
      const before = snapshot(path);

      await expect(
        rebaseBranch(await repos.open("rebase-over-staged"), "refs/heads/main", "side"),
      ).rejects.toThrow(
        "Git won't rebase while changes are staged, to a b.txt and f.txt. Commit or stash them, then rebase.",
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("over an untracked file in the way of the other branch's", async () => {
      const path = createSideRepo("rebase-over-untracked", CLEAN);
      writeFileSync(join(path, "m.txt"), "mine\n");
      const before = snapshot(path);

      await expect(
        rebaseBranch(await repos.open("rebase-over-untracked"), "refs/heads/main", "side"),
      ).rejects.toThrow(
        new RebaseBlockedError(
          "Rebasing onto main would overwrite the untracked file m.txt. Move or remove it, then rebase.",
        ),
      );
      expect(snapshot(path)).toEqual(before);
      expect(readFileSync(join(path, "m.txt"), "utf8")).toBe("mine\n");
    });

    it("while a merge is under way", async () => {
      const path = createDivergedRepo("rebase-during-merge", TEXT_CONFLICT);
      git(path, "branch", "other", "main~");
      tryGit(path, "merge", "-q", "side");

      await expect(
        rebaseBranch(await repos.open("rebase-during-merge"), "refs/heads/other", "main"),
      ).rejects.toThrow(
        new RebaseBlockedError("A merge is under way. Finish or abort it, then rebase."),
      );
    });

    it("while a rebase is under way", async () => {
      const path = createSideRepo("rebase-during-rebase", TEXT_CONFLICT);
      git(path, "branch", "other", "main");
      const repo = await repos.open("rebase-during-rebase");
      await rebaseBranch(repo, "refs/heads/main", "side");
      // Resolved, but not continued yet: still under way.
      await resolveText(repo, "one\nboth\nthree\n");

      await expect(rebaseBranch(repo, "refs/heads/other", "side")).rejects.toThrow(
        new RebaseBlockedError("A rebase is under way. Finish or abort it, then rebase."),
      );
      expect(await getOperation(repo)).toMatchObject({ kind: "rebase", branch: "side" });
    });

    it("while a popped stash's conflicts are left to resolve", async () => {
      const path = createSideRepo("rebase-during-pop", CLEAN);
      writeFileSync(join(path, "f.txt"), "stashed\n");
      git(path, "stash", "push", "-q");
      commitFiles(path, { "f.txt": "committed\n" }, "second");
      const repo = await repos.open("rebase-during-pop");
      await popStash(repo, git(path, "rev-parse", "refs/stash")).catch(() => undefined);
      const before = snapshot(path);

      await expect(rebaseBranch(repo, "refs/heads/main", "side")).rejects.toThrow(
        new RebaseBlockedError("Some files have conflicts. Resolve them, then rebase."),
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("with HEAD detached", async () => {
      const path = createSideRepo("rebase-detached", CLEAN);
      git(path, "checkout", "-q", "--detach");
      const before = snapshot(path);

      await expect(
        rebaseBranch(await repos.open("rebase-detached"), "refs/heads/main", "side"),
      ).rejects.toThrow(
        new RebaseBlockedError("HEAD is detached: check out a branch to rebase it."),
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("on a branch without commits yet", async () => {
      const path = createRepo("rebase-unborn");
      commitFiles(path, { "f.txt": "one\n" }, "first");
      git(path, "checkout", "-q", "--orphan", "empty");
      git(path, "rm", "-rqf", ".");

      await expect(
        rebaseBranch(await repos.open("rebase-unborn"), "refs/heads/main", "empty"),
      ).rejects.toThrow(
        new RebaseBlockedError(
          "empty has no commits yet, so there's nothing to rebase. Merge main into it instead.",
        ),
      );
      expect(git(path, "branch", "--show-current")).toBe("empty");
    });

    it("once another branch is checked out than the one the user saw", async () => {
      const path = createSideRepo("rebase-switched", CLEAN);
      git(path, "checkout", "-qb", "topic");
      const before = snapshot(path);

      const error = await rejection(
        rebaseBranch(await repos.open("rebase-switched"), "refs/heads/main", "side"),
      );
      expect(error).toBeInstanceOf(RepositoryChangedError);
      expect((error as Error).message).toBe(
        "Switched from side to topic meanwhile, so nothing was rebased.",
      );
      expect(snapshot(path)).toEqual(before);
    });

    it("once the branch is gone", async () => {
      const path = createSideRepo("rebase-gone", CLEAN);
      git(path, "branch", "other", "main");
      git(path, "branch", "-D", "other");

      await expect(
        rebaseBranch(await repos.open("rebase-gone"), "refs/heads/other", "side"),
      ).rejects.toThrow(new RebaseBlockedError("other no longer exists."));
    });

    it("onto itself", async () => {
      const path = createSideRepo("rebase-itself", CLEAN);
      const before = snapshot(path);

      await expect(
        rebaseBranch(await repos.open("rebase-itself"), "refs/heads/side", "side"),
      ).rejects.toThrow(new RebaseBlockedError("Can't rebase side onto itself."));
      expect(snapshot(path)).toEqual(before);
    });

    it("onto a tag", async () => {
      createSideRepo("rebase-onto-tag", CLEAN);
      await expect(
        rebaseBranch(await repos.open("rebase-onto-tag"), "refs/tags/v1", "side"),
      ).rejects.toThrow(new RebaseBlockedError("refs/tags/v1 isn't a branch."));
    });
  });

  it("is undone when a commit can't be signed, saying why", async () => {
    const path = createSideRepo("rebase-unsigned", CLEAN);
    const before = snapshot(path);
    const gpg = join(path, ".git", "no-gpg");
    writeFileSync(gpg, "#!/bin/sh\nexit 1\n");
    chmodSync(gpg, 0o755);
    git(path, "config", "commit.gpgSign", "true");
    // With gpg, whatever the user's global config signs with (ssh, say): `gpg.program` is gpg's.
    git(path, "config", "gpg.format", "openpgp");
    git(path, "config", "gpg.program", gpg);
    const repo = await repos.open("rebase-unsigned");

    const error = await rejection(rebaseBranch(repo, "refs/heads/main", "side"));
    expect(error).toBeInstanceOf(RebaseUndoneError);
    expect((error as Error).message).toMatch(
      /^Rebasing side onto main stopped partway, and was undone:\n[^]*failed to write commit object\nFix that, then rebase again\.$/,
    );
    expect(await getOperation(repo)).toBeNull();
    expect(snapshot(path)).toEqual(before);

    git(path, "config", "commit.gpgSign", "false");
    expect(await rebaseBranch(repo, "refs/heads/main", "side")).toBe("rebased");
    expect(git(path, "log", "--format=%s", "main..side")).toBe("theirs");
  });

  it("is left under way when it stops partway and can't be undone either", async () => {
    const path = createSideRepo("rebase-stuck", CLEAN);
    // Fails to sign, and takes away what aborting the rebase needs, as a crash might.
    const gpg = join(path, ".git", "no-gpg");
    writeFileSync(
      gpg,
      `#!/bin/sh\nrm -f "${join(path, ".git", "rebase-merge", "orig-head")}"\nexit 1\n`,
    );
    chmodSync(gpg, 0o755);
    git(path, "config", "commit.gpgSign", "true");
    git(path, "config", "gpg.format", "openpgp");
    git(path, "config", "gpg.program", gpg);
    const repo = await repos.open("rebase-stuck");

    const error = await rejection(rebaseBranch(repo, "refs/heads/main", "side"));
    expect(error).toBeInstanceOf(RebaseStoppedError);
    expect((error as Error).message).toMatch(
      /^Rebasing side onto main stopped partway:\n[^]*failed to write commit object\nFinish the rebase in a terminal, or abort it\.$/,
    );
    expect(await getOperation(repo)).toMatchObject({ kind: "rebase", branch: "side" });
  });

  it("moves the branch to the other when the other has its commits already", async () => {
    const path = createSideRepo("rebase-upstream-already", CLEAN);
    git(path, "checkout", "-q", "main");
    git(path, "cherry-pick", "side");
    git(path, "checkout", "-q", "side");

    expect(
      await rebaseBranch(await repos.open("rebase-upstream-already"), "refs/heads/main", "side"),
    ).toBe("fast-forward");
    expect(git(path, "rev-parse", "side")).toBe(git(path, "rev-parse", "main"));
  });

  it("says what git said when it failed otherwise", async () => {
    const path = createSideRepo("rebase-otherwise", CLEAN);
    git(path, "config", "rebase.backend", "no-such-backend");

    const error = await rejection(
      rebaseBranch(await repos.open("rebase-otherwise"), "refs/heads/main", "side"),
    );
    expect(error).toBeInstanceOf(GitError);
    expect((error as Error).message).toMatch(/no-such-backend/);
  });
});

describe("what git says when it refuses to rebase over uncommitted changes", () => {
  it("is read, for unstaged and staged ones", () => {
    expect(
      parseUncommitted(
        "error: cannot rebase: You have unstaged changes.\nerror: Please commit or stash them.\n",
      ),
    ).toBe("unstaged");
    expect(
      parseUncommitted(
        "error: cannot rebase: Your index contains uncommitted changes.\nerror: Please commit or stash them.\n",
      ),
    ).toBe("staged");
    expect(parseUncommitted("fatal: invalid upstream 'nope'\n")).toBeUndefined();
  });
});
