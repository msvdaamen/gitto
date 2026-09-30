import { describe, expect, it } from "vitest";

import { computeGraph } from "./graph";

describe("computeGraph", () => {
  it("keeps a linear history in a single lane", () => {
    const graph = computeGraph([
      { sha: "c", parents: ["b"] },
      { sha: "b", parents: ["a"] },
      { sha: "a", parents: [] },
    ]);

    expect(graph).toEqual([["●"], ["●"], ["●"]]);
  });

  it("opens a lane for a merged branch and closes it at the fork point", () => {
    const graph = computeGraph([
      { sha: "merge", parents: ["main", "side"] },
      { sha: "main", parents: ["base"] },
      { sha: "side", parents: ["base"] },
      { sha: "base", parents: [] },
    ]);

    expect(graph).toEqual([["●"], ["●", "│"], ["│", "●"], ["●", " "]]);
  });
});
