/**
 * The scroll containers of a repository's page, by their `data-scroll-restoration-id`: each starts
 * at the top again when another repository is shown. They stay on the page when switching, and the
 * router keeps such an element scrolled to where it was, unless it's told to scroll it to the top.
 */
export const SCROLL_IDS = [
  "history",
  "details",
  "unstaged-files",
  "staged-files",
  "sidebar-local-branches",
  "sidebar-remotes",
  "sidebar-tags",
  "sidebar-stashes",
  "sidebar-worktrees",
] as const;

export type ScrollId = (typeof SCROLL_IDS)[number];

/** What the router scrolls to the top when going somewhere new (its `scrollToTopSelectors`). */
export const SCROLL_TO_TOP = SCROLL_IDS.map((id) => `[data-scroll-restoration-id="${id}"]`);
