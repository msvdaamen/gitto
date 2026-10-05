import { describe, expect, it } from "vitest";

import { GitVersion, isSupportedVersion, parseGitVersion } from "./version";

describe("parseGitVersion", () => {
  it("reads the version from what each platform's git prints", () => {
    expect(parseGitVersion("git version 2.43.0\n")).toBe("2.43.0");
    expect(parseGitVersion("git version 2.45.1.windows.1\n")).toBe("2.45.1");
    expect(parseGitVersion("git version 2.39.3 (Apple Git-146)\n")).toBe("2.39.3");
    expect(parseGitVersion("git version 2.50.0.rc1\n")).toBe("2.50.0");
    expect(parseGitVersion("something else")).toBeNull();
  });
});

describe("isSupportedVersion", () => {
  it("accepts 2.41 and newer", () => {
    expect(isSupportedVersion("2.41")).toBe(true);
    expect(isSupportedVersion("2.41.0")).toBe(true);
    expect(isSupportedVersion("2.51.2")).toBe(true);
    expect(isSupportedVersion("3.0.0")).toBe(true);
    expect(isSupportedVersion("2.40.1")).toBe(false);
    expect(isSupportedVersion("2.39.5")).toBe(false);
    expect(isSupportedVersion("2.4.0")).toBe(false);
    expect(isSupportedVersion("1.99.0")).toBe(false);
  });
});

describe("GitVersion", () => {
  it("checks a supported git only once", async () => {
    let reads = 0;
    const version = new GitVersion(async () => {
      reads++;
      return "git version 2.43.0\n";
    });
    await version.check();
    await version.require();
    expect(reads).toBe(1);
  });

  it("checks an unsupported git again, until it's been updated", async () => {
    const outputs = ["git version 2.39.3\n", null, "git version 2.41.1\n"];
    let reads = 0;
    const version = new GitVersion(async () => outputs[reads++] ?? null);

    await expect(version.check()).resolves.toMatchObject({ version: "2.39.3", supported: false });
    await expect(version.require()).rejects.toThrow("Gitto couldn't find Git.");
    await expect(version.check()).resolves.toMatchObject({ version: "2.41.1", supported: true });
    await version.require();
    expect(reads).toBe(3);
  });

  it("doesn't refuse a git whose version it can't read", async () => {
    const version = new GitVersion(async () => "git version unknown\n");
    await expect(version.check()).resolves.toMatchObject({ supported: true });
  });

  it("runs the installed git", async () => {
    await expect(new GitVersion().check()).resolves.toMatchObject({
      version: expect.stringMatching(/^\d+\.\d+/),
      required: "2.41",
    });
  });
});
