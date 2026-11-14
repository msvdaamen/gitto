import { describe, expect, it } from "vitest";

import { inFolder, parentFolder, repositoryName } from "./clone";

describe("repositoryName", () => {
  it("names the repository after the end of its URL, without .git", () => {
    expect(repositoryName("https://github.com/owner/gitto.git")).toBe("gitto");
    expect(repositoryName("https://github.com/owner/gitto")).toBe("gitto");
    expect(repositoryName("https://github.com/owner/gitto/")).toBe("gitto");
    expect(repositoryName("https://github.com/owner/gitto.git/")).toBe("gitto");
    expect(repositoryName("https://gitlab.com/group/sub/gitto.git?ref=main#x")).toBe("gitto");
    expect(repositoryName("git@github.com:owner/gitto.git")).toBe("gitto");
    expect(repositoryName("git@example.com:gitto.git")).toBe("gitto");
    expect(repositoryName("ssh://git@example.com:2222/owner/gitto.git")).toBe("gitto");
    expect(repositoryName("/srv/git/gitto.git")).toBe("gitto");
    expect(repositoryName("C:\\repos\\gitto")).toBe("gitto");
    expect(repositoryName("  file:///srv/git/my.repo.git  ")).toBe("my.repo");
  });

  it("is empty until there's a name to go by", () => {
    expect(repositoryName("")).toBe("");
    expect(repositoryName("https://")).toBe("");
    expect(repositoryName("git@github.com")).toBe("");
    expect(repositoryName("git@github.com:")).toBe("");
    expect(repositoryName(".git")).toBe("");
  });
});

describe("parentFolder", () => {
  it("is the folder a path is in", () => {
    expect(parentFolder("/home/me/code/gitto")).toBe("/home/me/code");
    expect(parentFolder("/home/me/code/gitto/")).toBe("/home/me/code");
    expect(parentFolder("/gitto")).toBe("/");
    expect(parentFolder("C:\\code\\gitto")).toBe("C:\\code");
    expect(parentFolder("C:\\gitto")).toBe("C:\\");
    expect(parentFolder("gitto")).toBeUndefined();
  });
});

describe("inFolder", () => {
  it("puts a name in a folder, with its separator", () => {
    expect(inFolder("/home/me/code", "gitto")).toBe("/home/me/code/gitto");
    expect(inFolder("/home/me/code/", "gitto")).toBe("/home/me/code/gitto");
    expect(inFolder("/", "gitto")).toBe("/gitto");
    expect(inFolder("C:\\code", "gitto")).toBe("C:\\code\\gitto");
  });
});
