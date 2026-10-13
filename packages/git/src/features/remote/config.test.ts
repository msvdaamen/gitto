import { describe, expect, it } from "vitest";

import { makesMergeCommit, mapRef, parseConfig, rebases, remotesToFetch } from "./config";

/** `git config -z --get-regexp`'s output for `entries`, each a key and a value, or a key alone. */
function output(...entries: [key: string, value?: string][]): string {
  return entries
    .map(([key, value]) => (value === undefined ? key : `${key}\n${value}`) + "\0")
    .join("");
}

describe("parseConfig", () => {
  it("takes the last value of a key set several times, as git does, but keeps them all", () => {
    const config = parseConfig(output(["pull.ff", "true"], ["pull.ff", "only"]));
    expect(config.get("pull.ff")).toBe("only");
    expect(config.all("pull.ff")).toEqual(["true", "only"]);
    expect(config.has("pull.ff")).toBe(true);
    expect(config.get("pull.rebase")).toBeUndefined();
    expect(config.all("pull.rebase")).toEqual([]);
  });

  it("takes a boolean set without a value for true", () => {
    expect(parseConfig(output(["pull.rebase"])).get("pull.rebase")).toBe("true");
  });

  it("keeps a branch's name and a URL as they are, with no output being no settings", () => {
    const config = parseConfig(output(["branch.Feature.merge", "refs/heads/Feature"]));
    expect(config.get("branch.Feature.merge")).toBe("refs/heads/Feature");
    expect([...parseConfig("").keys()]).toEqual([]);
  });
});

describe("rebases and makesMergeCommit", () => {
  it("rebase unless the setting is false, with the branch's own setting first", () => {
    expect(rebases(parseConfig(""), "main")).toBe(false);
    expect(rebases(parseConfig(output(["pull.rebase", "true"])), "main")).toBe(true);
    expect(rebases(parseConfig(output(["pull.rebase", "merges"])), "main")).toBe(true);
    expect(rebases(parseConfig(output(["pull.rebase", "No"])), "main")).toBe(false);
    const own = parseConfig(output(["pull.rebase", "true"], ["branch.main.rebase", "false"]));
    expect(rebases(own, "main")).toBe(false);
    expect(rebases(own, "other")).toBe(true);
  });

  it("only fast-forward wins over rebasing, and makes no merge commit", () => {
    const ffOnly = parseConfig(output(["pull.rebase", "true"], ["pull.ff", "only"]));
    expect(rebases(ffOnly, "main")).toBe(false);
    expect(makesMergeCommit(ffOnly, "main")).toBe(false);
    expect(makesMergeCommit(parseConfig(output(["pull.rebase", "true"])), "main")).toBe(false);
    expect(makesMergeCommit(parseConfig(""), "main")).toBe(true);
  });
});

describe("remotesToFetch", () => {
  it("is every remote but the one given, and those set to be skipped", () => {
    const config = parseConfig(
      output(
        ["remote.origin.url", "https://example.com/a.git"],
        ["remote.fork.url", "https://example.com/b.git"],
        ["remote.mirror.url", "https://example.com/c.git"],
        ["remote.mirror.skipfetchall", "true"],
        ["remote.other.url", "https://example.com/d.git"],
        ["remote.other.skipfetchall", "off"],
      ),
    );
    expect(remotesToFetch(config, "origin")).toEqual(["fork", "other"]);
    expect(remotesToFetch(config, "none")).toEqual(["origin", "fork", "other"]);
  });
});

describe("mapRef", () => {
  it("maps a ref through fetch refspecs, as git does", () => {
    const fetch = ["+refs/heads/*:refs/remotes/origin/*"];
    expect(mapRef(fetch, "refs/heads/main")).toBe("refs/remotes/origin/main");
    expect(mapRef(fetch, "refs/tags/v1")).toBeUndefined();
    expect(
      mapRef(["refs/heads/main:refs/remotes/origin/main"], "refs/heads/feature"),
    ).toBeUndefined();
    // Left out by a negative refspec, wherever it is.
    expect(mapRef([...fetch, "^refs/heads/wip/*"], "refs/heads/wip/x")).toBeUndefined();
    // Without a destination, a fetch keeps it nowhere.
    expect(mapRef(["refs/heads/main"], "refs/heads/main")).toBeUndefined();
  });

  it("maps the branch checked out through push refspecs, as git does", () => {
    expect(mapRef(["refs/heads/*:refs/for/*"], "refs/heads/main", true)).toBe("refs/for/main");
    expect(mapRef(["HEAD:refs/for/main"], "refs/heads/x", true)).toBe("refs/for/main");
    expect(mapRef(["main:trunk"], "refs/heads/main", true)).toBe("refs/heads/trunk");
    expect(mapRef(["refs/heads/main"], "refs/heads/main", true)).toBe("refs/heads/main");
    expect(mapRef([":"], "refs/heads/x", true)).toBe("refs/heads/x");
    expect(mapRef(["refs/heads/main:refs/heads/main"], "refs/heads/x", true)).toBeUndefined();
  });
});
