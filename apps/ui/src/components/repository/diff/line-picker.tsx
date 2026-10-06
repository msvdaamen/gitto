// Picking lines of the patch on show to stage or unstage (see `PatchViewer`'s `staging`): selecting
// them with the mouse or the keyboard, by line or a hunk at a time, and the buttons that stage
// them. Which lines a selection picks, and the rows to move it between, is `line-staging.ts`'s.
import type { LineSelection } from "@gitto/git/types";
import type { FileDiffMetadata, FileDiffOptions, SelectedLineRange } from "@pierre/diffs";
import { cn } from "cn";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import { createSignal, onCleanup } from "solid-js";

import type { DiffStyle } from "@/hooks/diff-style";

import {
  changedLines,
  changedRows,
  hunkButtons,
  rangeOf,
  selectedLines,
  toSelection,
  type ChangedLine,
  type HunkButton,
  type RowOf,
} from "./line-staging";
import type { ViewerFileDiff } from "./viewer-runtime";

/** Staging or unstaging lines of the patch on show, which can then be picked (see `PatchViewer`). */
export interface LineStaging {
  action: "stage" | "unstage";
  /** Lines are being staged: no others are until the patch that leaves is on show. */
  busy: boolean;
  /**
   * Stages `lines` of `patch`, the patch on show they were picked from; resolves once they're
   * staged, and rejects if they couldn't be.
   */
  onStage: (patch: string, lines: LineSelection) => Promise<void>;
}

/** What picking lines needs of the viewer: its view, and what's on show in it. */
export interface LinePickerDeps {
  /** The scroll container: it has the keyboard, and is scrolled to show the lines selected. */
  root: HTMLDivElement;
  /** The library's diff, made with the options picking adds (see `options`): after the picker. */
  diffs: () => ViewerFileDiff;
  diffStyle: () => DiffStyle;
  staging: () => LineStaging | undefined;
  /** Whether lines can be picked to stage: only of a patch that isn't being edited. */
  pickable: () => boolean;
  /** Whether the patch on show is the one lines are picked from (see `PatchViewer`'s `isLive`). */
  isLive: () => boolean;
  /** The patch on show, as git gave it, which the lines picked are staged from; none until. */
  shownPatch: () => string | undefined;
  /** Whether another patch is on its way, to take the place of the one on show. */
  awaitingPatch: () => boolean;
}

/** Picking lines of the patch on show (see `createLinePicker`). */
export interface LinePicker {
  /** What picking lines adds to the viewer's options. */
  options: () => FileDiffOptions<HunkButton, undefined>;
  /** The hunks' buttons for `diff`, if lines can be picked from it. */
  buttons: (diff: FileDiffMetadata) => ReturnType<typeof hunkButtons>;
  /**
   * `diff`, the patch's, is on show: in place of the last file's, or of the same file's with
   * `sameFile`. Selects the change in place of the lines the keyboard staged, if it did.
   */
  shown: (diff: FileDiffMetadata, sameFile: boolean) => void;
}

/**
 * Picks lines to stage in the patch on show, in `deps.root` (see `PatchViewer`'s `staging`): by
 * selecting them, with the mouse or with the arrow keys (Shift to extend), or a hunk at a time,
 * with its button or `[` and `]`. The button in the gutter, by the changed line the pointer is on
 * or the last one selected, or Space stages them; Esc clears the selection. Once the keyboard
 * staged some, the selection moves to the change that's in their place.
 */
export function createLinePicker(deps: LinePickerDeps): LinePicker {
  const { root, pickable } = deps;
  /** The changed lines of the diff on show, and the rows selected, if any. */
  let lines: ChangedLine[] = [];
  const [selection, setSelection] = createSignal<SelectedLineRange | null>(null);
  /** The changed lines selected. */
  const [picked, setPicked] = createSignal<ChangedLine[]>([]);
  /** Whether the selection is of whole hunks, gone through with `[` and `]`, not by line. */
  let byHunk = false;
  /**
   * Where the keyboard staged lines from, by the line numbers of the side the patch leaves as it
   * is (the working tree's, staging; HEAD's, unstaging): the change found there in the next patch
   * on show is selected (see `shown`), or its hunk, if they were picked a hunk at a time; if they
   * couldn't be staged, the same lines are, in the patch refetched.
   */
  let stagedAt: { side: "old" | "new"; position: number; hunks: boolean } | undefined;
  /** The row a line is on in the diff on show, unified or side by side. */
  const rowOf: RowOf = (lineNumber, side) =>
    deps.diffs().getLineIndex(lineNumber, side)?.[deps.diffStyle() === "split" ? 1 : 0];

  const select = (range: SelectedLineRange | null) => {
    setSelection(range);
    byHunk = false;
    setPicked(range ? selectedLines(lines, range, rowOf) : []);
  };

  /** Selects the changes of `line`'s hunk, to go through them a hunk at a time. */
  const selectHunk = (line: ChangedLine) => {
    const hunk = changedRows(lines, rowOf).filter((row) => row.line.hunk === line.hunk);
    deps.diffs().setSelectedLines(rangeOf(hunk[0]!.line, hunk.at(-1)!.line));
    byHunk = true;
    return hunk;
  };

  /** Stages `changed`, of the patch on show, if lines can be staged now. */
  const stage = async (changed: ChangedLine[], byKeyboard: boolean) => {
    const staging = deps.staging();
    if (!staging || staging.busy || !deps.isLive() || changed.length === 0) return;
    const side: "new" | "old" = staging.action === "stage" ? "new" : "old";
    const at = byKeyboard
      ? { side, position: Math.min(...changed.map((line) => line[side])), hunks: byHunk }
      : undefined;
    stagedAt = at;
    try {
      await staging.onStage(deps.shownPatch()!, toSelection(changed));
    } catch {
      // Not staged, e.g. as the file changed since: the patch refetched is selected in the same
      // place, if another is on its way, and the selection stays where it is otherwise.
      if (stagedAt === at && !deps.awaitingPatch()) stagedAt = undefined;
    }
  };

  /**
   * The changed line the pointer is on, if any, which the gutter's button stages without a
   * selection. Forgotten once another patch is on show, where it may be another line, until the
   * pointer moves.
   */
  const [hovered, setHovered] = createSignal<ChangedLine>();
  /** What the gutter's button stages: the lines selected, or else the line the pointer is on. */
  const toStage = () => (selection() ? picked() : [hovered()].filter((line) => line !== undefined));

  const unstaging = () => deps.staging()?.action === "unstage";
  const stageLabel = () =>
    `${unstaging() ? "Unstage" : "Stage"} ${selection() ? "lines (Space)" : "line"}`;

  /**
   * The button in the gutter, by the number of the last line selected or else the changed line
   * the pointer is on, which stages them.
   */
  const stageButton = (
    <button
      type="button"
      aria-label={stageLabel()}
      title={stageLabel()}
      class={cn(
        "absolute top-px left-0 z-10 flex size-[calc(var(--diffs-line-height)-2px)] -translate-x-[calc(100%+4px)] cursor-pointer items-center justify-center rounded-[4px] bg-primary text-bg shadow-[0_1px_4px_rgba(0,0,0,.25)] hover:bg-primary-strong focus-ring group-data-busy:cursor-default group-data-busy:opacity-50",
        toStage().length === 0 && "hidden",
      )}
      // Not a click on the line it's on, which would select that line alone: heard on the button
      // itself, before the library does in the diff, rather than delegated to the document.
      on:pointerdown={(event) => event.stopPropagation()}
      onClick={() => void stage(toStage(), false)}
    >
      {unstaging() ? <Minus size={14} strokeWidth={2.5} /> : <Plus size={14} strokeWidth={2.5} />}
    </button>
  ) as HTMLElement;

  /** A hunk's button, above its first change. */
  const hunkButton = (hunk: number) => {
    const label = deps.staging()?.action === "unstage" ? "Unstage hunk" : "Stage hunk";
    return (
      <div class="flex justify-end px-2 py-0.5 font-sans">
        <button
          type="button"
          class="cursor-pointer rounded-md border border-border bg-panel-raised px-2 py-px text-[11px] font-[600] text-text-soft hover:bg-panel-hover hover:text-text focus-ring group-data-busy:cursor-default group-data-busy:opacity-50"
          on:pointerdown={(event) => event.stopPropagation()}
          onClick={() =>
            void stage(
              lines.filter((line) => line.hunk === hunk),
              false,
            )
          }
        >
          {label}
        </button>
      </div>
    ) as HTMLElement;
  };

  const options = (): FileDiffOptions<HunkButton, undefined> => {
    const can = pickable();
    return {
      enableLineSelection: can,
      enableGutterUtility: can,
      renderGutterUtility: () => stageButton,
      // Clicking a line gives the view the keyboard, to go on from there.
      onLineSelectionStart: () => root.focus({ preventScroll: true }),
      onLineSelected: select,
      onLineEnter: ({ lineNumber, annotationSide }) =>
        setHovered(
          lines.find((line) => line.side === annotationSide && line.lineNumber === lineNumber),
        ),
      onLineLeave: () => setHovered(undefined),
      renderAnnotation: ({ metadata }) => (metadata ? hunkButton(metadata.hunk) : undefined),
    };
  };
  const buttons = (diff: FileDiffMetadata) => (pickable() ? hunkButtons(diff) : []);

  const shown = (diff: FileDiffMetadata, sameFile: boolean) => {
    lines = changedLines(diff);
    setHovered(undefined);
    reselect(sameFile);
  };

  /**
   * Selects the change in place of the lines the keyboard staged, in the patch they left, so the
   * next is a key away: the first one after where they were, on the side that didn't change, or
   * its hunk if they were picked a hunk at a time. In another file (the next one, once the last
   * lines were staged), its first change, or hunk.
   */
  const reselect = (sameFile: boolean) => {
    const at = stagedAt;
    stagedAt = undefined;
    if (!at || !pickable()) return;
    const rows = changedRows(lines, rowOf);
    const target = sameFile
      ? (rows.find(({ line }) => line[at.side] >= at.position) ?? rows.at(-1))
      : rows[0];
    if (!target) return;
    if (at.hunks) selectHunk(target.line);
    else deps.diffs().setSelectedLines(rangeOf(target.line));
    reveal(target.line);
  };

  /** Scrolls the view, if it has to, to show `line`'s row. */
  const reveal = (line: ChangedLine) => {
    const diffs = deps.diffs();
    const position = diffs.getLinePosition(line.lineNumber, line.side);
    if (!position) return;
    const top = position.top + (diffs.top ?? 0);
    // A few rows around it stay in sight, as when moving through a list.
    const margin = position.height * 3;
    if (top - margin < root.scrollTop) root.scrollTop = Math.max(0, top - margin);
    else if (top + position.height + margin > root.scrollTop + root.clientHeight) {
      root.scrollTop = top + position.height + margin - root.clientHeight;
    }
  };

  /** The first changed row from the top of the view, or the last one above its bottom. */
  const inView = (down: boolean) => {
    const diffs = deps.diffs();
    const rows = changedRows(lines, rowOf);
    const topOf = ({ line }: (typeof rows)[number]) =>
      (diffs.getLinePosition(line.lineNumber, line.side)?.top ?? 0) + (diffs.top ?? 0);
    return down
      ? (rows.find((row) => topOf(row) >= root.scrollTop) ?? rows.at(-1))
      : (rows.findLast((row) => topOf(row) < root.scrollTop + root.clientHeight) ?? rows[0]);
  };

  /** The rows the selection spans, top and bottom, and the one it was extended to. */
  const selectedRows = (range: SelectedLineRange) => {
    const start = rowOf(range.start, range.side ?? "additions") ?? 0;
    const end = rowOf(range.end, range.endSide ?? range.side ?? "additions") ?? 0;
    return { top: Math.min(start, end), bottom: Math.max(start, end), end };
  };

  /**
   * Selects the next changed row down or up from the selection, or extends the selection to it;
   * without a selection, the first one in view.
   */
  const move = (down: boolean, extend: boolean) => {
    const rows = changedRows(lines, rowOf);
    const selected = selection();
    let target: (typeof rows)[number] | undefined;
    if (!selected) target = inView(down);
    else {
      const { top, bottom, end } = selectedRows(selected);
      const from = extend ? end : down ? bottom : top;
      target = down ? rows.find(({ row }) => row > from) : rows.findLast(({ row }) => row < from);
    }
    if (!target) return;
    deps
      .diffs()
      .setSelectedLines(
        extend && selected
          ? { ...selected, end: target.line.lineNumber, endSide: target.line.side }
          : rangeOf(target.line),
      );
    reveal(target.line);
  };

  /** Selects the next hunk's changes down or up from the selection, or the first one in view. */
  const moveToHunk = (down: boolean) => {
    const rows = changedRows(lines, rowOf);
    const selected = selection();
    let target: ChangedLine | undefined;
    if (!selected) target = inView(down)?.line;
    else {
      const { top, bottom } = selectedRows(selected);
      // The first row of each hunk, down from the selection, or up from its top.
      const firsts = rows.filter(({ line }, i) => i === 0 || rows[i - 1]!.line.hunk !== line.hunk);
      target = (
        down ? firsts.find(({ row }) => row > bottom) : firsts.findLast(({ row }) => row < top)
      )?.line;
    }
    if (!target) return;
    const hunk = selectHunk(target);
    reveal(hunk.at(-1)!.line);
    reveal(hunk[0]!.line);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (!pickable() || !deps.isLive() || event.defaultPrevented) return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    // A button's own keys: a hunk's, or the one to stage the lines selected.
    if (event.target instanceof HTMLButtonElement) return;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp":
        move(event.key === "ArrowDown", event.shiftKey);
        break;
      case "]":
      case "[":
        moveToHunk(event.key === "]");
        break;
      case " ":
        // Without lines selected, Space scrolls, as it does elsewhere.
        if (picked().length === 0) return;
        void stage(picked(), true);
        break;
      case "Escape":
        // Without a selection, Esc goes on to close the changes.
        if (!selection()) return;
        deps.diffs().setSelectedLines(null);
        break;
      default:
        return;
    }
    event.preventDefault();
  };
  root.addEventListener("keydown", onKeyDown);
  onCleanup(() => root.removeEventListener("keydown", onKeyDown));

  return { options, buttons, shown };
}
