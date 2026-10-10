import { describe, expect, it } from "vitest";

import { channelOf, compareVersions, isPrerelease } from "./version.ts";

describe("versions", () => {
  it("are on the nightly channel with a nightly part, else on the release channel", () => {
    expect(channelOf("1.2.4-nightly29853462")).toBe("nightly");
    expect(channelOf("1.2.3")).toBe("release");
    expect(channelOf("1.3.0-beta.1")).toBe("release");
    expect(channelOf("0.0.0")).toBe("release");
  });

  it("are pre-releases with a pre-release part", () => {
    expect(isPrerelease("1.2.4-nightly29853462")).toBe(true);
    expect(isPrerelease("1.3.0-beta.1")).toBe(true);
    expect(isPrerelease("1.2.3")).toBe(false);
    expect(isPrerelease("1.2.3+build.1")).toBe(false);
  });

  it("are ordered as semver orders them", () => {
    const ordered = [
      "0.9.9",
      "1.2.3",
      "1.2.4-nightly29853462",
      "1.2.4-nightly29853463",
      "1.2.4-nightly99930239",
      "1.2.4",
      "1.2.10",
      "1.3.0-alpha",
      "1.3.0-alpha.1",
      "1.3.0-alpha.beta",
      "1.3.0-beta.2",
      "1.3.0-beta.11",
      "1.3.0-nightly29853462",
      "1.3.0-rc.1",
      "1.3.0-rc.2",
      "1.3.0",
    ];
    for (const [i, a] of ordered.entries()) {
      for (const [j, b] of ordered.entries()) {
        expect(Math.sign(compareVersions(a, b)), `${a} vs ${b}`).toBe(Math.sign(i - j));
      }
    }
    expect(compareVersions("1.2.3+build.1", "1.2.3")).toBe(0);
  });
});
