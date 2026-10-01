import { describe, expect, it } from "vitest";

import type { Repo } from "../../core/repo";
import { getHosting, parseHosting } from "./remote";

describe("parseHosting", () => {
  it("reads GitHub remotes over SSH and HTTPS", () => {
    const github = { provider: "github", owner: "msvdaamen", name: "gitto" };
    expect(parseHosting("git@github.com:msvdaamen/gitto.git")).toEqual(github);
    expect(parseHosting("https://github.com/msvdaamen/gitto")).toEqual(github);
    expect(parseHosting("https://token@github.com/msvdaamen/gitto.git/")).toEqual(github);
    expect(parseHosting("ssh://git@github.com/msvdaamen/gitto.git")).toEqual(github);
  });

  it("reads GitLab remotes, including nested groups and self-hosted instances", () => {
    expect(parseHosting("git@gitlab.com:group/sub/project.git")).toEqual({
      provider: "gitlab",
      host: "gitlab.com",
      project: "group/sub/project",
    });
    expect(parseHosting("ssh://git@gitlab.example.com:2222/team/app.git")).toEqual({
      provider: "gitlab",
      host: "gitlab.example.com",
      project: "team/app",
    });
  });

  it("knows nothing of other hosts and local paths", () => {
    expect(parseHosting("git@bitbucket.org:team/app.git")).toBeUndefined();
    expect(parseHosting("/srv/git/app.git")).toBeUndefined();
    expect(parseHosting("file:///srv/git/app.git")).toBeUndefined();
    expect(parseHosting("https://github.com/msvdaamen")).toBeUndefined();
  });
});

function repoWithRemotes(config: string): Repo {
  return { read: async () => config } as unknown as Repo;
}

describe("getHosting", () => {
  it("prefers origin, then the first remote on a known host", async () => {
    const remotes = [
      "remote.fork.url git@github.com:someone/gitto.git",
      "remote.origin.url git@github.com:msvdaamen/gitto.git",
    ].join("\n");
    expect(await getHosting(repoWithRemotes(remotes))).toEqual({
      provider: "github",
      owner: "msvdaamen",
      name: "gitto",
    });

    const unknownOrigin = [
      "remote.origin.url /srv/git/gitto.git",
      "remote.mirror.url https://gitlab.com/msvdaamen/gitto.git",
    ].join("\n");
    expect(await getHosting(repoWithRemotes(unknownOrigin))).toMatchObject({ provider: "gitlab" });
  });

  it("has none without remotes", async () => {
    const repo = { read: () => Promise.reject(new Error("exit 1")) } as unknown as Repo;
    expect(await getHosting(repo)).toBeUndefined();
  });
});
