import { mkdirSync, mkdtempSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { copyDependencies, findDependencies } from "./native-deps";

let root: string;

function pkg(dir: string, name: string, fields: object = {}) {
  const path = join(dir, "node_modules", name);
  mkdirSync(path, { recursive: true });
  writeFileSync(join(path, "package.json"), JSON.stringify({ name, ...fields }));
  return path;
}

afterEach(() => rmSync(root, { recursive: true, force: true }));

describe("native dependencies", () => {
  it("finds a package and its dependencies, skipping uninstalled optional ones", async () => {
    root = realpathSync(mkdtempSync(join(tmpdir(), "gitto-deps-")));
    const app = join(root, "app");
    const watcher = pkg(app, "watcher", {
      dependencies: { glob: "1" },
      optionalDependencies: { "watcher-linux": "1", "watcher-darwin": "1" },
    });
    // Nested, like pnpm's layout, and found from the package that needs it.
    const glob = pkg(watcher, "glob");
    const linux = pkg(root, "watcher-linux");

    const found = await findDependencies(["watcher"], app);
    expect(found).toEqual(
      new Map([
        ["watcher", watcher],
        ["glob", glob],
        ["watcher-linux", linux],
      ]),
    );

    const build = join(root, "build");
    await copyDependencies(found, build);
    expect(readdirSync(join(build, "node_modules")).toSorted()).toEqual([
      "glob",
      "watcher",
      "watcher-linux",
    ]);
  });

  it("fails on a missing dependency", async () => {
    root = realpathSync(mkdtempSync(join(tmpdir(), "gitto-deps-")));
    pkg(root, "watcher", { dependencies: { glob: "1" } });
    await expect(findDependencies(["watcher"], root)).rejects.toThrow("Can't find glob");
  });
});
