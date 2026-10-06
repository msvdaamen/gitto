// The buttons over each conflict in the viewer (see `ConflictViewer`): elements of the page, in the
// library's rows for them, rather than of the diff's shadow root, so they're styled like the rest
// of it.
import type { MergeConflictResolution } from "@pierre/diffs";

import { markerLabel, type ConflictAction } from "./conflict-diff";

/** How a conflict is resolved: ours, theirs, or both, ours first. */
export type Resolution = MergeConflictResolution;

const ROW_CLASS =
  "flex min-w-0 items-center gap-1 py-[3px] pl-2 font-sans text-[11.5px] text-muted data-current:text-text-soft data-current:shadow-[inset_2px_0_0_var(--primary)]";

const BUTTON_CLASS =
  "h-[22px] cursor-pointer rounded-[5px] border border-border bg-panel-raised px-2 text-[11.5px] font-[600] text-text-soft enabled:hover:border-[color-mix(in_srgb,var(--primary)_45%,var(--border))] enabled:hover:text-text focus-ring disabled:cursor-default disabled:opacity-50";

/**
 * The row of buttons over the conflict `action` is of: Keep ours, Keep theirs and Keep both, then
 * the labels git gave its sides, e.g. `HEAD ⟷ feature`. Marked as the current one if `current`,
 * and turned off unless `enabled`, as the library draws it again whenever it sees fit, e.g. once
 * it's highlighted (see `markConflictRows` for the ones already drawn).
 */
export function renderConflictRow(
  action: ConflictAction,
  state: { current: boolean; enabled: boolean },
  onResolve: (resolution: Resolution) => void,
): HTMLElement {
  const ours = markerLabel(action.markerLines.start);
  const theirs = markerLabel(action.markerLines.end);
  const row = document.createElement("div");
  row.dataset.conflict = String(action.conflictIndex);
  row.className = ROW_CLASS;
  row.toggleAttribute("data-current", state.current);
  const choices: [Resolution, string, string][] = [
    ["current", "Keep ours", ours ? `Keep ours: ${ours}` : "Keep ours"],
    ["incoming", "Keep theirs", theirs ? `Keep theirs: ${theirs}` : "Keep theirs"],
    ["both", "Keep both", "Keep both, ours first"],
  ];
  for (const [resolution, label, title] of choices) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.title = title;
    button.className = BUTTON_CLASS;
    button.disabled = !state.enabled;
    button.addEventListener("click", () => onResolve(resolution));
    row.append(button);
  }
  const sides = document.createElement("span");
  sides.className = "ml-1.5 truncate text-faint";
  sides.textContent = ours || theirs ? `${ours || "ours"} ⟷ ${theirs || "theirs"}` : "";
  row.append(sides);
  return row;
}

/**
 * Marks the row of the conflict `current` (by its index) among those drawn in `container`, and
 * turns all their buttons on or off.
 */
export function markConflictRows(
  container: HTMLElement,
  current: number | undefined,
  enabled: boolean,
): void {
  for (const row of container.querySelectorAll<HTMLElement>("[data-conflict]")) {
    row.toggleAttribute("data-current", row.dataset.conflict === String(current));
    for (const button of row.querySelectorAll("button")) button.disabled = !enabled;
  }
}

/** Scrolls the row of the conflict `index` (drawn in `container`) to the middle of the view. */
export function scrollToConflict(container: HTMLElement, index: number): void {
  container
    .querySelector<HTMLElement>(`[data-conflict="${index}"]`)
    ?.scrollIntoView({ block: "center" });
}
