import { describe, expect, it } from "vitest";

import { squirrelVersion } from "./squirrel-version";

const INT32_MAX = 2 ** 31 - 1;

/** Squirrel's comparison (its NuGet's SemanticVersion.CompareTo), for versions with the same numbers. */
function compareSquirrel(a: string, b: string) {
  const [aPre, bPre] = [a, b].map((version) => version.slice(version.indexOf("-") + 1));
  const [aMatch, bMatch] = [aPre!, bPre!].map((pre) => /([a-z]+)([0-9]+)$/i.exec(pre));
  if (aMatch && bMatch && aMatch[1]!.toLowerCase() === bMatch[1]!.toLowerCase()) {
    const [aNumber, bNumber] = [Number(aMatch[2]), Number(bMatch[2])];
    // Its int.Parse throws past this.
    expect(Math.max(aNumber, bNumber)).toBeLessThanOrEqual(INT32_MAX);
    return Math.sign(aNumber - bNumber);
  }
  return aPre!.toLowerCase() < bPre!.toLowerCase() ? -1 : 1;
}

describe("Squirrel's version", () => {
  it("is a nightly's minutes from 1970 to when it was built, in at most 20 characters", () => {
    expect(squirrelVersion("1.2.4-nightly.20261005134259")).toBe("1.2.4-nightly29853462");
    expect(squirrelVersion("1.3.0-rc.1.nightly.20261005134259")).toBe("1.3.0-nightly29853462");
    expect(squirrelVersion("1.2.4-nightly.21591231235959")).toBe("1.2.4-nightly99930239");
  });

  it("sorts nightlies as they were built, as Squirrel compares them and as text", () => {
    const built = [
      "1.3.0-nightly.20261005134259",
      "1.3.0-nightly.20261005134359",
      "1.3.0-beta.1.nightly.20270101000000",
      "1.3.0-rc.1.nightly.20500101000000",
      "1.3.0-rc.1.nightly.21591231235959",
    ].map(squirrelVersion);
    for (const [i, a] of built.entries()) {
      for (const [j, b] of built.entries()) {
        if (i === j) continue;
        expect(compareSquirrel(a, b), `${a} vs ${b}`).toBe(Math.sign(i - j));
        expect(a < b, `${a} vs ${b} as text`).toBe(i < j);
      }
    }
  });

  it("keeps any other version", () => {
    expect(squirrelVersion("1.2.3")).toBe("1.2.3");
    expect(squirrelVersion("1.3.0-beta.1")).toBe("1.3.0-beta.1");
    expect(squirrelVersion("0.0.0")).toBe("0.0.0");
  });
});
