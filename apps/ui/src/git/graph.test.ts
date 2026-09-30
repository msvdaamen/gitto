import { describe, expect, it } from "vitest";

import { computeGraph } from "./graph";

describe("computeGraph", () => {
  it("keeps a linear history in a single lane", () => {
    const graph = computeGraph([
      { sha: "c", parents: ["b"] },
      { sha: "b", parents: ["a"] },
      { sha: "a", parents: [] },
    ]);

    expect(graph).toEqual([
      { column: 0, through: [], top: [], bottom: [{ from: 0, to: 0 }], width: 1 },
      {
        column: 0,
        through: [],
        top: [{ from: 0, to: 0 }],
        bottom: [{ from: 0, to: 0 }],
        width: 1,
      },
      { column: 0, through: [], top: [{ from: 0, to: 0 }], bottom: [], width: 1 },
    ]);
  });

  it("curves a merged branch in from its own lane and back at the fork point", () => {
    const [merge, main, side, base] = computeGraph([
      { sha: "merge", parents: ["main", "side"] },
      { sha: "main", parents: ["base"] },
      { sha: "side", parents: ["base"] },
      { sha: "base", parents: [] },
    ]);

    // The merge opens a lane for the merged branch.
    expect(merge!.bottom).toEqual([
      { from: 0, to: 0 },
      { from: 0, to: 1 },
    ]);
    expect(main).toMatchObject({ column: 0, through: [1] });
    expect(side).toMatchObject({ column: 1, through: [0] });
    // Both lanes lead to the fork point, where the side branch's lane curves back in.
    expect(base).toMatchObject({
      column: 0,
      top: [
        { from: 0, to: 0 },
        { from: 1, to: 0 },
      ],
      width: 2,
    });
  });

  it("gives a branch that isn't merged its own lane from its tip", () => {
    const [feature, main, base] = computeGraph([
      { sha: "feature", parents: ["base"] },
      { sha: "main", parents: ["base"] },
      { sha: "base", parents: [] },
    ]);

    expect(feature).toMatchObject({ column: 0, top: [] });
    expect(main).toMatchObject({ column: 1, top: [], through: [0] });
    expect(base!.top).toEqual([
      { from: 0, to: 0 },
      { from: 1, to: 0 },
    ]);
  });

  it("joins a merged parent that already has a lane instead of opening another", () => {
    const [feature, merge] = computeGraph([
      { sha: "feature", parents: ["side"] },
      { sha: "merge", parents: ["main", "side"] },
      { sha: "side", parents: ["main"] },
      { sha: "main", parents: [] },
    ]);

    expect(feature).toMatchObject({ column: 0 });
    expect(merge).toMatchObject({ column: 1, through: [0] });
    expect(merge!.bottom).toEqual([
      { from: 1, to: 1 },
      { from: 1, to: 0 },
    ]);
  });

  it("doesn't reuse a lane that ends at a node for that node's merged parent", () => {
    const rows = computeGraph([
      { sha: "a", parents: ["m"] },
      { sha: "b", parents: ["m"] },
      { sha: "m", parents: ["x", "y"] },
      { sha: "y", parents: ["x"] },
      { sha: "x", parents: [] },
    ]);

    // Lane 1 ends at `m`, so its merged parent `y` goes to lane 2, not lane 1.
    expect(rows[2]!.bottom).toContainEqual({ from: 0, to: 2 });
  });
});
