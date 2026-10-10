import { describe, expect, it } from "vitest";

import {
  addVersion,
  bumpFor,
  type Change,
  type Changelog,
  parseCommit,
  parseLog,
  parseTitle,
  previousTag,
  releaseNotes,
  staleNightlies,
  userFacingChanges,
  type VersionChanges,
} from "./changelog.ts";

function at(version: string): VersionChanges {
  return { version, date: "2026-10-10", changes: [] };
}

function change(overrides: Partial<Change>): Change {
  return { type: "feat", breaking: false, description: "add it", commit: "abc", ...overrides };
}

describe("titles", () => {
  it("are a type, an optional scope and !, and a description", () => {
    expect(parseTitle("feat: add worktrees")).toEqual({
      ok: true,
      type: "feat",
      breaking: false,
      description: "add worktrees",
    });
    expect(parseTitle("fix(diff)!: keep the scroll")).toEqual({
      ok: true,
      type: "fix",
      scope: "diff",
      breaking: true,
      description: "keep the scroll",
    });
  });

  it("are refused otherwise", () => {
    for (const title of [
      "Add worktrees",
      "feat:add worktrees",
      "feat: ",
      "Feat: add worktrees",
      "feature: add worktrees",
      "feat(): add worktrees",
    ]) {
      expect(parseTitle(title).ok, title).toBe(false);
    }
  });
});

describe("commits", () => {
  it("are read with their pull request, breaking footer and Changelog line", () => {
    const message = [
      "feat(sidebar): add worktrees (#74)",
      "",
      "A Worktrees section lists them.",
      "",
      "Changelog: Open a worktree by double-clicking it.",
      "BREAKING CHANGE: the old setting is gone",
    ].join("\n");
    expect(parseCommit("abc", message)).toEqual({
      type: "feat",
      scope: "sidebar",
      breaking: true,
      description: "add worktrees",
      detail: "Open a worktree by double-clicking it.",
      pr: 74,
      commit: "abc",
    });
  });

  it("are left out without a Conventional Commits title", () => {
    expect(parseCommit("abc", "Add worktrees (#74)")).toBeNull();
    expect(parseCommit("abc", 'Revert "feat: add worktrees"')).toBeNull();
  });

  it("are read from git log's output", () => {
    const log =
      "a1\x1ffix: one (#2)\n\x1e\nb2\x1fNot conventional\x1e\nc3\x1fci: three\n\nbody\n\x1e\n";
    expect(parseLog(log)).toEqual([
      { type: "fix", breaking: false, description: "one", pr: 2, commit: "a1" },
      { type: "ci", breaking: false, description: "three", commit: "c3" },
    ]);
  });
});

describe("the changelog", () => {
  it("adds a version in order, replacing one it had, and keeps the newest", () => {
    let changelog: Changelog = { versions: [at("1.2.4-nightly2"), at("1.2.4-nightly1")] };
    changelog = addVersion(changelog, at("1.2.4-nightly3"));
    expect(changelog.versions.map((v) => v.version)).toEqual([
      "1.2.4-nightly3",
      "1.2.4-nightly2",
      "1.2.4-nightly1",
    ]);
    changelog = addVersion(changelog, { ...at("1.2.4-nightly3"), date: "2026-10-11" }, 2);
    expect(changelog.versions).toEqual([
      { ...at("1.2.4-nightly3"), date: "2026-10-11" },
      at("1.2.4-nightly2"),
    ]);
  });

  it("bumps the major for a breaking change, the minor for a feature, else the patch", () => {
    expect(bumpFor([change({ type: "fix" }), change({ type: "fix", breaking: true })])).toBe(
      "major",
    );
    expect(bumpFor([change({ type: "fix" }), change({ type: "feat" })])).toBe("minor");
    expect(bumpFor([change({ type: "fix" }), change({ type: "chore" })])).toBe("patch");
    expect(bumpFor([])).toBe("patch");
  });
});

describe("the previous tag", () => {
  const tags = [
    "nightly",
    "v1.2.3",
    "v1.2.4-nightly29853462",
    "v1.2.4-nightly29854900",
    "v1.3.0-rc.1",
    "v1.3.0",
    "v1.4.0-beta.1",
    "other",
    "v2",
  ];

  it("is the last nightly's for a nightly", () => {
    expect(previousTag("1.2.4-nightly29860000", tags)).toBe("v1.2.4-nightly29854900");
    expect(previousTag("1.4.1-nightly29870000", tags)).toBe("v1.2.4-nightly29854900");
    expect(previousTag("1.2.4-nightly29853462", tags)).toBeUndefined();
  });

  it("is the release before it for a release, leaving out pre-releases", () => {
    expect(previousTag("1.3.0", tags)).toBe("v1.2.3");
    expect(previousTag("1.4.0", tags)).toBe("v1.3.0");
    expect(previousTag("1.2.3", tags)).toBeUndefined();
  });

  it("is the version before it for a pre-release, leaving out nightlies", () => {
    expect(previousTag("1.3.0-rc.2", tags)).toBe("v1.3.0-rc.1");
    expect(previousTag("1.4.0-beta.2", tags)).toBe("v1.4.0-beta.1");
    expect(previousTag("1.2.5-beta.1", tags)).toBe("v1.2.3");
  });
});

describe("stale nightlies", () => {
  it("are the nightlies past the newest kept", () => {
    const tags = ["v1.2.3", "v1.2.4-nightly3", "v1.2.4-nightly1", "v1.2.4-nightly2", "v1.3.0-rc.1"];
    expect(staleNightlies(tags, 2)).toEqual(["v1.2.4-nightly1"]);
    expect(staleNightlies(tags, 30)).toEqual([]);
  });
});

describe("release notes", () => {
  it("group every change, breaking ones first", () => {
    const notes = releaseNotes([
      change({ description: "add worktrees", pr: 74, detail: "Open one by double-clicking." }),
      change({ type: "fix", scope: "diff", description: "keep the scroll" }),
      change({ type: "chore", description: "bump electron" }),
      change({ type: "refactor", breaking: true, description: "drop the old setting" }),
      change({ type: "perf", description: "load faster" }),
    ]);
    expect(notes).toBe(
      [
        "### ⚠️ Breaking changes",
        "",
        "- Drop the old setting",
        "",
        "### New",
        "",
        "- Add worktrees (#74)",
        "  Open one by double-clicking.",
        "",
        "### Fixes",
        "",
        "- **diff:** Keep the scroll",
        "",
        "### Performance",
        "",
        "- Load faster",
        "",
        "### Other changes",
        "",
        "- chore: bump electron",
      ].join("\n"),
    );
  });

  it("say so when nothing changed", () => {
    expect(releaseNotes([])).toBe("Nothing changed since the last build.");
  });
});

describe("what users see", () => {
  const changelog: Changelog = {
    versions: [
      { ...at("1.2.4-nightly4"), changes: [change({ type: "chore" })] },
      { ...at("1.2.4-nightly3"), changes: [change({ type: "fix" }), change({ type: "ci" })] },
      { ...at("1.2.4-nightly2"), changes: [change({ type: "refactor", breaking: true })] },
      { ...at("1.2.4-nightly1"), changes: [change({ type: "perf" })] },
    ],
  };

  it("is the user-facing changes of the versions since the last seen, up to this one", () => {
    expect(
      userFacingChanges(changelog, { after: "1.2.4-nightly1", upTo: "1.2.4-nightly3" }),
    ).toEqual([
      { ...at("1.2.4-nightly3"), changes: [change({ type: "fix" })] },
      { ...at("1.2.4-nightly2"), changes: [change({ type: "refactor", breaking: true })] },
    ]);
  });

  it("leaves out versions with none, and is every version without a last seen", () => {
    expect(userFacingChanges(changelog, { upTo: "1.2.4-nightly4" }).map((v) => v.version)).toEqual([
      "1.2.4-nightly3",
      "1.2.4-nightly2",
      "1.2.4-nightly1",
    ]);
    expect(
      userFacingChanges(changelog, { after: "1.2.4-nightly3", upTo: "1.2.4-nightly4" }),
    ).toEqual([]);
  });
});
