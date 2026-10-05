import { describe, expect, it } from "vitest";

import { remoteHost, remoteProvider } from "./remote";

describe("remoteHost", () => {
  it("reads the host of URLs and scp-like addresses", () => {
    expect(remoteHost("https://github.com/owner/repo.git")).toBe("github.com");
    expect(remoteHost("https://user:token@GitLab.com/owner/repo")).toBe("gitlab.com");
    expect(remoteHost("ssh://git@git.example.com:2222/owner/repo.git")).toBe("git.example.com");
    expect(remoteHost("git@github.com:owner/repo.git")).toBe("github.com");
    expect(remoteHost("example.com:repo.git")).toBe("example.com");
  });

  it("has none for a repository on this machine", () => {
    expect(remoteHost("/Users/ada/upstream")).toBeNull();
    expect(remoteHost("../upstream")).toBeNull();
    expect(remoteHost("file:///Users/ada/upstream")).toBeNull();
    expect(remoteHost(String.raw`C:\Users\ada\upstream`)).toBeNull();
  });
});

describe("remoteProvider", () => {
  it("names known hosts by their service", () => {
    expect(remoteProvider("git@github.com:owner/repo.git")).toBe("GitHub");
    expect(remoteProvider("ssh://git@ssh.github.com:443/owner/repo.git")).toBe("GitHub");
    expect(remoteProvider("https://gitlab.com/owner/repo")).toBe("GitLab");
  });

  it("names others by their host", () => {
    expect(remoteProvider("https://git.example.com/repo.git")).toBe("git.example.com");
    expect(remoteProvider("/Users/ada/upstream")).toBe("Local");
  });
});
