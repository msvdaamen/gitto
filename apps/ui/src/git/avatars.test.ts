import { describe, expect, it } from "vitest";

import { avatarUrl } from "./avatars";

describe("avatarUrl", () => {
  it("looks up the Gravatar for the trimmed, lowercase address, asking for a 404 if there's none", async () => {
    // SHA-256 of "ada@example.com".
    const hash = "b5fc85e55755f9e0d030a10ab4429b6b2944855f9a0d60077fe832becbc41d72";
    expect(await avatarUrl(" Ada@Example.com ")).toBe(
      `https://www.gravatar.com/avatar/${hash}?s=80&d=404`,
    );
  });

  it("uses the GitHub avatar for a GitHub noreply address", async () => {
    expect(await avatarUrl("12345+ada@users.noreply.github.com")).toBe(
      "https://avatars.githubusercontent.com/u/12345?s=80&v=4",
    );
    expect(await avatarUrl("ada@users.noreply.github.com")).toBe(
      "https://github.com/ada.png?size=80",
    );
  });

  it("has none without an email address", async () => {
    expect(await avatarUrl("")).toBeUndefined();
  });
});
