import { describe, expect, it } from "vitest";

import { parseRefs } from "./parse";

describe("parseRefs", () => {
  it("reads branches, remotes and tags, skipping remote HEADs", () => {
    const output = [
      ["refs/heads/main", "aaa", "origin/main", "ahead 2, behind 1", "*"],
      ["refs/heads/feature/x", "bbb", "", "", " "],
      ["refs/remotes/origin/HEAD", "aaa", "", "", " "],
      ["refs/remotes/origin/main", "ccc", "", "", " "],
      ["refs/tags/v1.0.0", "ddd", "", "", " "],
    ]
      .map((fields) => fields.join("\0"))
      .join("\n");

    expect(parseRefs(output)).toEqual([
      {
        name: "main",
        fullName: "refs/heads/main",
        kind: "local",
        sha: "aaa",
        current: true,
        upstream: "origin/main",
        ahead: 2,
        behind: 1,
      },
      expect.objectContaining({ name: "feature/x", kind: "local", current: false, upstream: null }),
      expect.objectContaining({ name: "origin/main", kind: "remote" }),
      expect.objectContaining({ name: "v1.0.0", kind: "tag" }),
    ]);
  });
});
