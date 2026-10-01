import { describe, expect, it } from "vitest";

import { avatarColor, compactCount, initials, relativeTime } from "./format";

describe("initials", () => {
  it("takes the first and last word's first letters", () => {
    expect(initials("Ada King Lovelace")).toBe("AL");
    expect(initials("ada.lovelace")).toBe("AL");
  });

  it("takes the first two letters of a single word", () => {
    expect(initials("ada")).toBe("AD");
  });
});

describe("avatarColor", () => {
  it("picks the same colour for the same key, whatever its case", () => {
    expect(avatarColor("Ada@Example.com")).toBe(avatarColor("ada@example.com"));
  });
});

describe("relativeTime", () => {
  const now = Date.UTC(2026, 0, 31);

  it("uses the largest unit that fits", () => {
    expect(relativeTime(now - 3 * 24 * 60 * 60 * 1000, now)).toBe("3 days ago");
    expect(relativeTime(now - 2 * 60 * 60 * 1000, now)).toBe("2 hours ago");
  });

  it("says just now for less than a minute", () => {
    expect(relativeTime(now - 30_000, now)).toBe("Just now");
  });
});

describe("compactCount", () => {
  it("shortens thousands", () => {
    expect(compactCount(999)).toBe("999");
    expect(compactCount(1234)).toBe("1.2k");
    expect(compactCount(2000)).toBe("2k");
    expect(compactCount(12_345)).toBe("12k");
  });
});
