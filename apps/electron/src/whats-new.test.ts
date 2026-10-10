import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { Change, Changelog } from "@gitto/release/changelog";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Changes, readChangelog } from "./whats-new";

let dir: string;
let lastSeenFile: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "gitto-whats-new-"));
  lastSeenFile = join(dir, "last-seen-version");
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
  vi.restoreAllMocks();
});

const feat: Change = { type: "feat", breaking: false, description: "add it", commit: "a" };
const chore: Change = { type: "chore", breaking: false, description: "tidy", commit: "b" };

const changelog: Changelog = {
  versions: [
    { version: "1.2.4-nightly3", date: "2026-10-12", changes: [feat] },
    { version: "1.2.4-nightly2", date: "2026-10-11", changes: [chore] },
    { version: "1.2.4-nightly1", date: "2026-10-10", changes: [feat] },
  ],
};

function lastSeen() {
  return readFileSync(lastSeenFile, "utf8");
}

describe("what's new", () => {
  it("is nothing on the first run, which is remembered as seen", async () => {
    const changes = new Changes("1.2.4-nightly3", changelog, lastSeenFile);
    expect(await changes.unseen()).toEqual([]);
    expect(lastSeen()).toBe("1.2.4-nightly3");
  });

  it("is what users see of every version since the last seen, until it's seen", async () => {
    writeFileSync(lastSeenFile, "1.2.4-nightly1\n");
    const changes = new Changes("1.2.4-nightly3", changelog, lastSeenFile);

    expect(await changes.unseen()).toEqual([changelog.versions[0]]);
    expect(lastSeen()).toBe("1.2.4-nightly1\n");
    await changes.seen();
    expect(await changes.unseen()).toEqual([]);
  });

  it("is nothing after only internal changes, or going back a version, which count as seen", async () => {
    writeFileSync(lastSeenFile, "1.2.4-nightly1");
    expect(await new Changes("1.2.4-nightly2", changelog, lastSeenFile).unseen()).toEqual([]);
    expect(lastSeen()).toBe("1.2.4-nightly2");

    writeFileSync(lastSeenFile, "1.2.4-nightly3");
    expect(await new Changes("1.2.4-nightly1", changelog, lastSeenFile).unseen()).toEqual([]);
    expect(lastSeen()).toBe("1.2.4-nightly1");
  });

  it("is nothing for a build of one's own, which isn't remembered", async () => {
    const changes = new Changes("0.0.0", changelog, lastSeenFile);
    expect(await changes.unseen()).toEqual([]);
    await changes.seen();
    expect(() => lastSeen()).toThrow();
  });

  it("lists every version up to this one", () => {
    const changes = new Changes("1.2.4-nightly2", changelog, lastSeenFile);
    expect(changes.all().map((v) => v.version)).toEqual(["1.2.4-nightly1"]);
  });
});

describe("the changelog", () => {
  it("is read, or none when the build has none or it's broken", () => {
    const path = join(dir, "changelog.json");
    expect(readChangelog(path)).toEqual({ versions: [] });

    writeFileSync(path, JSON.stringify(changelog));
    expect(readChangelog(path)).toEqual(changelog);

    vi.spyOn(console, "error").mockImplementation(() => {});
    writeFileSync(path, "{");
    expect(readChangelog(path)).toEqual({ versions: [] });
    expect(console.error).toHaveBeenCalled();
  });
});
