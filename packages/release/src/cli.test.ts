import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { Changelog } from "./changelog.ts";

const CLI = join(import.meta.dirname, "cli.ts");
let repo: string;

beforeEach(() => {
  repo = mkdtempSync(join(tmpdir(), "gitto-release-"));
  git("init", "--quiet", "--initial-branch=main");
});

afterEach(() => rmSync(repo, { recursive: true, force: true }));

function git(...args: string[]) {
  return execFileSync("git", args, {
    cwd: repo,
    encoding: "utf8",
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: "Test",
      GIT_AUTHOR_EMAIL: "test@example.com",
      GIT_COMMITTER_NAME: "Test",
      GIT_COMMITTER_EMAIL: "test@example.com",
    },
  });
}

function commit(message: string) {
  git("commit", "--quiet", "--allow-empty", "-m", message);
}

function cli(...args: string[]) {
  const result = spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
  return { status: result.status, stdout: result.stdout.trim(), stderr: result.stderr.trim() };
}

describe("the changelog script", () => {
  it("adds a nightly's changes since the last nightly to its changelog", () => {
    commit("feat: add worktrees (#1)");
    git("tag", "v0.0.1-nightly1");
    commit("fix(diff): keep the scroll (#2)");
    commit("Not conventional");
    commit("chore: bump electron (#3)");
    const previous: Changelog = {
      versions: [{ version: "0.0.1-nightly1", date: "2026-10-09", changes: [] }],
    };
    writeFileSync(join(repo, "previous.json"), JSON.stringify(previous));

    expect(cli("previous", "0.0.1-nightly2").stdout).toBe("v0.0.1-nightly1");
    const generated = cli(
      "generate",
      "--version",
      "0.0.1-nightly2",
      "--from",
      "v0.0.1-nightly1",
      "--previous",
      "previous.json",
      "--out",
      "changelog.json",
      "--notes",
      "notes.md",
    );
    expect(generated.status, generated.stderr).toBe(0);

    const changelog = JSON.parse(readFileSync(join(repo, "changelog.json"), "utf8")) as Changelog;
    expect(changelog.versions.map((v) => v.version)).toEqual(["0.0.1-nightly2", "0.0.1-nightly1"]);
    expect(changelog.versions[0]!.changes.map((c) => [c.type, c.description, c.pr])).toEqual([
      ["chore", "bump electron", 3],
      ["fix", "keep the scroll", 2],
    ]);
    expect(readFileSync(join(repo, "notes.md"), "utf8")).toContain(
      "- **diff:** Keep the scroll (#2)",
    );
  });

  it("counts a release's changes from the last release, and says what it bumps", () => {
    commit("feat: add worktrees");
    git("tag", "v1.2.3");
    commit("fix: keep the scroll");
    git("tag", "v1.3.0-rc.1");
    commit("fix: keep the selection");

    expect(cli("previous", "1.2.4").stdout).toBe("v1.2.3");
    expect(cli("bump", "v1.2.3").stdout).toBe("patch");
    expect(cli("bump").stdout).toBe("minor");
    commit("feat!: drop the old setting");
    expect(cli("bump", "v1.2.3").stdout).toBe("major");
  });

  it("lists the nightlies past the newest kept", () => {
    commit("feat: add worktrees");
    for (const tag of ["v1.2.3", "v1.2.4-nightly1", "v1.2.4-nightly2", "v1.2.4-nightly3"]) {
      git("tag", tag);
    }
    expect(cli("stale", "2").stdout).toBe("v1.2.4-nightly1");
    expect(cli("stale", "0").status).toBe(1);
  });

  it("checks a pull request's title", () => {
    expect(cli("check-title", "feat: add worktrees").status).toBe(0);
    const refused = cli("check-title", "Add worktrees");
    expect(refused.status).toBe(1);
    expect(refused.stderr).toContain("<type>: <description>");
  });
});
