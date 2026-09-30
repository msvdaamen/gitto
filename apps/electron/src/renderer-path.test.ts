import { join, resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { resolveRendererPath } from "./renderer-path";

const rendererDir = resolve("/app/renderer/main_window");

describe("resolveRendererPath", () => {
  it("resolves files in the renderer build", () => {
    expect(resolveRendererPath(rendererDir, "/index.html")).toBe(join(rendererDir, "index.html"));
    expect(resolveRendererPath(rendererDir, "/assets/my%20file.js")).toBe(
      join(rendererDir, "assets", "my file.js"),
    );
  });

  it("rejects paths that escape the renderer build", () => {
    expect(resolveRendererPath(rendererDir, "/../main.js")).toBeNull();
    expect(resolveRendererPath(rendererDir, "/%2e%2e/main.js")).toBeNull();
    expect(resolveRendererPath(rendererDir, "/assets/..%2F..%2Fmain.js")).toBeNull();
  });
});
