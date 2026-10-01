import type { Ref } from "@gitto/git/types";
import { describe, expect, it } from "vitest";

import { buildRefTree, flattenRefTree } from "./ref-tree";
import type { RefTreeNode } from "./ref-tree";

function ref(fullName: string, name: string, kind: Ref["kind"]): Ref {
  return { name, fullName, kind, sha: "", current: false, upstream: null, ahead: 0, behind: 0 };
}

const local = (name: string) => ref(`refs/heads/${name}`, name, "local");
const remote = (name: string) => ref(`refs/remotes/${name}`, name, "remote");

/** The tree as nested names, with a trailing `/` on folders. */
function shape(nodes: RefTreeNode[]): unknown[] {
  return nodes.map((node) =>
    node.type === "folder" ? { [`${node.name}/`]: shape(node.children) } : node.name,
  );
}

describe("buildRefTree", () => {
  it("keeps branches without a slash at the top level", () => {
    expect(shape(buildRefTree([local("main"), local("dev")]))).toEqual(["main", "dev"]);
  });

  it("groups branches on the part before each slash, folders first", () => {
    const tree = buildRefTree([
      local("main"),
      local("feature/login"),
      local("fix/crash"),
      local("feature/ui/button"),
    ]);

    expect(shape(tree)).toEqual([
      { "feature/": [{ "ui/": ["button"] }, "login"] },
      { "fix/": ["crash"] },
      "main",
    ]);
  });

  it("nests remote branches under their remote", () => {
    const tree = buildRefTree([remote("origin/main"), remote("origin/feature/login")]);

    expect(shape(tree)).toEqual([{ "origin/": [{ "feature/": ["login"] }, "main"] }]);
  });

  it("gives folders their full ref path and a count of the refs below them", () => {
    const [origin] = buildRefTree([remote("origin/main"), remote("origin/feature/login")]);

    expect(origin).toMatchObject({ path: "refs/remotes/origin", count: 2 });
    expect(origin?.type === "folder" && origin.children[0]).toMatchObject({
      path: "refs/remotes/origin/feature",
      count: 1,
    });
  });

  it("keeps a branch and a folder with the same name apart", () => {
    expect(shape(buildRefTree([local("feature"), local("feature/login")]))).toEqual([
      { "feature/": ["login"] },
      "feature",
    ]);
  });
});

describe("flattenRefTree", () => {
  const tree = buildRefTree([
    local("main"),
    local("feature/login"),
    local("feature/ui/button"),
    local("fix/crash"),
  ]);
  const lines = (collapsed: string[]) =>
    flattenRefTree(tree, (path) => collapsed.includes(path)).map(
      ({ node, depth }) => `${"  ".repeat(depth)}${node.name}${node.type === "folder" ? "/" : ""}`,
    );

  it("lists folders before their children, indented by depth", () => {
    expect(lines([])).toEqual([
      "feature/",
      "  ui/",
      "    button",
      "  login",
      "fix/",
      "  crash",
      "main",
    ]);
  });

  it("leaves out what's inside collapsed folders", () => {
    expect(lines(["refs/heads/feature"])).toEqual(["feature/", "fix/", "  crash", "main"]);
    expect(lines(["refs/heads/feature/ui"])).toEqual([
      "feature/",
      "  ui/",
      "  login",
      "fix/",
      "  crash",
      "main",
    ]);
  });
});
