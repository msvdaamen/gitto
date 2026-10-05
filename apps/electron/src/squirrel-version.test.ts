import { describe, expect, it } from "vitest";

import { squirrelVersion } from "./squirrel-version";

describe("Squirrel's version", () => {
  it("is when a nightly was built, in at most 20 characters", () => {
    expect(squirrelVersion("1.2.4-nightly.20261005134259")).toBe("1.2.4-n20261005134259");
    expect(squirrelVersion("1.3.0-rc.1.nightly.20261005134259")).toBe("1.3.0-n20261005134259");
  });

  it("sorts nightlies as text in the order they were built", () => {
    const built = [
      "1.2.4-nightly.20261005134259",
      "1.2.4-nightly.20261006090000",
      "1.3.0-beta.1.nightly.20261007000000",
      "1.3.0-rc.1.nightly.20261008000000",
      "1.3.0-rc.1.nightly.20261008000001",
    ].map(squirrelVersion);
    // Squirrel compares the numbers first, then the pre-release part as text.
    const sorted = built.toSorted((a, b) => {
      const [aCore, aPre] = a.split("-");
      const [bCore, bPre] = b.split("-");
      return aCore!.localeCompare(bCore!, "en", { numeric: true }) || (aPre! < bPre! ? -1 : 1);
    });
    expect(sorted).toEqual(built);
  });

  it("keeps any other version", () => {
    expect(squirrelVersion("1.2.3")).toBe("1.2.3");
    expect(squirrelVersion("1.3.0-beta.1")).toBe("1.3.0-beta.1");
    expect(squirrelVersion("0.0.0")).toBe("0.0.0");
  });
});
