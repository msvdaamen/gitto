import { createRoot, createSignal } from "solid-js";
import { afterEach, describe, expect, it } from "vitest";

import { useCollapsed } from "./collapsed";

/** Runs the hook in its own reactive root, like a freshly mounted sidebar. */
function mount(repositoryId: () => string, defaults?: Record<string, boolean>) {
  return createRoot((dispose) => ({ ...useCollapsed(repositoryId, defaults), dispose }));
}

afterEach(() => localStorage.clear());

describe("useCollapsed", () => {
  it("falls back to the defaults, then to expanded", () => {
    const collapsed = mount(() => "repo", { tags: true });

    expect(collapsed.isCollapsed("tags")).toBe(true);
    expect(collapsed.isCollapsed("refs/heads/feature")).toBe(false);
    collapsed.dispose();
  });

  it("keeps toggled state across remounts", () => {
    const first = mount(() => "repo", { tags: true });
    first.toggle("tags");
    first.toggle("refs/heads/feature");
    first.dispose();

    const second = mount(() => "repo", { tags: true });
    expect(second.isCollapsed("tags")).toBe(false);
    expect(second.isCollapsed("refs/heads/feature")).toBe(true);
    second.dispose();
  });

  it("keeps a separate state per repository", () => {
    const [repositoryId, setRepositoryId] = createSignal("a");
    const collapsed = mount(repositoryId);

    collapsed.toggle("remotes");
    setRepositoryId("b");
    expect(collapsed.isCollapsed("remotes")).toBe(false);
    setRepositoryId("a");
    expect(collapsed.isCollapsed("remotes")).toBe(true);
    collapsed.dispose();
  });

  it("ignores unreadable saved state", () => {
    localStorage.setItem("gitto-sidebar-collapsed:repo", "not json");
    const collapsed = mount(() => "repo");

    expect(collapsed.isCollapsed("remotes")).toBe(false);
    collapsed.dispose();
  });
});
