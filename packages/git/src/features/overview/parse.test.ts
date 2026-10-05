import { describe, expect, it } from "vitest";

import { parseReflog, pickRemote } from "./parse";

/** `log -g -z` output for reflog messages, a second apart, each moving HEAD to commit "subject". */
function reflog(...messages: string[]) {
  return messages
    .flatMap((message, i) => [`HEAD@{${1_700_000_000 - i}}`, message, "subject"])
    .join("\0");
}

/** `config -z --get-regexp` output for remotes' URLs. */
const config = (...remotes: [string, string][]) =>
  remotes.map(([name, url]) => `remote.${name}.url\n${url}`).join("\0") + "\0";

const kinds = (output: string) =>
  parseReflog(output).map(({ kind, target }) => (target ? `${kind} ${target}` : kind));

describe("parseReflog", () => {
  it("reads when, what and git's message", () => {
    expect(parseReflog(reflog("commit: Fix it"))).toEqual([
      {
        at: 1_700_000_000_000,
        kind: "commit",
        target: null,
        subject: "subject",
        message: "commit: Fix it",
      },
    ]);
  });

  it("tells what was done and with what", () => {
    expect(
      kinds(
        reflog(
          "commit (initial): First",
          "commit (amend): Fix it",
          "commit (merge): Merge branch 'feature'",
          "checkout: moving from main to feature/x",
          "checkout: moving from main to 1a2b3c4d",
          "merge origin/main: Fast-forward",
          "pull --ff-only origin main: Fast-forward",
          "pull: Merge made by the 'ort' strategy.",
          "reset: moving to HEAD~1",
          "cherry-pick: Fix it",
          'revert: Revert "Fix it"',
          "clone: from https://example.com/repo.git",
          "Branch: renamed refs/heads/a to refs/heads/b",
        ),
      ),
    ).toEqual([
      "commit",
      "amend",
      "commit",
      "checkout feature/x",
      "checkout 1a2b3c4d",
      "merge origin/main",
      "pull origin main",
      "pull",
      "reset HEAD~1",
      "cherry-pick",
      "revert",
      "clone",
      "other",
    ]);
  });

  it("shows a rebase once it's done, not its steps", () => {
    expect(
      kinds(
        reflog(
          "rebase (finish): returning to refs/heads/feature",
          "rebase (pick): Fix it",
          "rebase (start): checkout main",
          "pull --rebase (finish): returning to refs/heads/main",
          "pull --rebase (pick): Fix it",
          "pull --rebase (start): checkout 1a2b3c4d",
        ),
      ),
    ).toEqual(["rebase feature", "pull"]);
  });

  it("leaves out what didn't move HEAD, or has no message", () => {
    expect(kinds(reflog("reset: moving to HEAD", "", "commit: Fix it"))).toEqual(["commit"]);
  });

  it("reads nothing from no output", () => {
    expect(parseReflog("")).toEqual([]);
  });
});

describe("pickRemote", () => {
  it("picks origin", () => {
    expect(pickRemote(config(["fork", "git@a:x"], ["origin", "git@b:y"]))).toEqual({
      name: "origin",
      url: "git@b:y",
    });
  });

  it("picks the first remote without origin, and its first URL", () => {
    expect(
      pickRemote(config(["up.stream", "git@a:x"], ["up.stream", "git@a:z"], ["fork", "git@b:y"])),
    ).toEqual({ name: "up.stream", url: "git@a:x" });
  });

  it("picks none without remotes", () => {
    expect(pickRemote("")).toBeNull();
  });
});
