import { describe, expect, it } from "vitest";

import { folderName } from "./init";

describe("folderName", () => {
  it("is the name, trimmed", () => {
    expect(folderName("  my project ")).toBe("my project");
    expect(folderName("gitto.app")).toBe("gitto.app");
    expect(folderName(".dotfiles")).toBe(".dotfiles");
  });

  it("is empty for a name that isn't one folder's", () => {
    expect(folderName("")).toBe("");
    expect(folderName("   ")).toBe("");
    expect(folderName(".")).toBe("");
    expect(folderName("..")).toBe("");
    expect(folderName("code/gitto")).toBe("");
    expect(folderName("code\\gitto")).toBe("");
  });
});
