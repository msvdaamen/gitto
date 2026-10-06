// The diff viewer, from @pierre/diffs. Loaded on its own (see `FileDiffView`), as it brings Shiki.
import type { LineSelection } from "@gitto/git/types";
import {
  getSharedHighlighter,
  hydratePartialDiff,
  parseDiffFromFile,
  parsePatchFiles,
  VirtualizedFileDiff,
  Virtualizer,
  type FileDiffLoadedFiles,
  type FileDiffMetadata,
  type FileDiffOptions,
  type HunkExpansionRegion,
  type PostRenderPhase,
  type SelectedLineRange,
} from "@pierre/diffs";
import type * as EditModule from "@pierre/diffs/edit";
import type { Editor } from "@pierre/diffs/edit";
import { getOrCreateWorkerPoolSingleton, type WorkerPoolManager } from "@pierre/diffs/worker";
import { cn } from "cn";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import { createEffect, createMemo, createSignal, on, onCleanup, untrack } from "solid-js";

import { useConnected } from "@/components/ui/virtual-list";
import type { DiffStyle } from "@/hooks/diff-style";
import { useTheme } from "@/hooks/theme";

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
import {
  carriedExpansion,
  fitToLines,
  fitToPatch,
  hasMatchingEnds,
  type Side,
} from "./patch-files";

/** One side of a file, read in full: with its version if it's the working tree's. */
export type LoadedFile = string | { contents: string; version: string };

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

/** Editing the file on show: what it started from, and what can be done with it meanwhile. */
export interface EditSession {
  /** The version of the file on disk the edits are of (see `readWorkingTreeFile`). */
  version: string;
  /** The file as the editing started. */
  text: string;
  /** Drops the edits made since the last save, and stops editing. */
  discard: () => void;
  /** Whether there's a selection for Esc to collapse, or several to make one, in the editor. */
  hasSelection: () => boolean;
}

let editorModule: Promise<typeof EditModule> | undefined;
/** The library's editor, loaded the first time a file's edited. */
const loadEditor = () => (editorModule ??= import("@pierre/diffs/edit"));

/** Highlighting runs in workers; one diff is on show at a time, so a couple is plenty. */
const WORKERS = 2;

/**
 * How many diffs the workers' manager keeps highlighted, in the main thread, for a quick return
 * to one shown lately, or prepared ahead (see `preparePatch`): the files a step away. The
 * library's default is a hundred, whatever their size, and a highlighted line takes 1-5KB: 40MB
 * for a file of 30,000 lines read in full to show more of it, 200MB for a change of 25,000 lines.
 * So the long ones are dropped once their view closes too (see `KEPT_LINES`).
 */
const CACHED_DIFFS = 16;

/**
 * Lines (of both sides) from which a diff's highlighting is dropped once the view that showed it
 * closes, rather than kept among the `CACHED_DIFFS`: a whole file read to show more lines, edits,
 * or a long patch. Shorter ones highlight again in well under the time the view waits for that
 * (see `HIGHLIGHT_WAIT_MS`), so they're kept for a quick return.
 */
const KEPT_LINES = 1000;

const THEMES = { dark: "pierre-dark", light: "pierre-light" } as const;

/**
 * The app's background and colours for added and removed lines, in place of the theme's, and the
 * focus ring on the buttons that show more lines (see `focusableExpandButtons`).
 */
const APP_CSS = `:host {
  --diffs-dark-bg: var(--bg);
  --diffs-light-bg: var(--bg);
  --diffs-addition-color-override: var(--mint);
  --diffs-deletion-color-override: var(--coral);
}
[data-expand-button]:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}`;

let pool: WorkerPoolManager | undefined;

/** The workers that highlight, started the first time they're needed. */
function workerPool(): WorkerPoolManager {
  if (pool) return pool;
  // The main thread has a highlighter of its own, which only draws plain text in the viewer (and
  // edits, in the library's editor): Shiki's JavaScript engine does, without loading and compiling
  // the WebAssembly one there. The first one asked for is the one that's shared, so it's this.
  void getSharedHighlighter({
    themes: [THEMES.dark, THEMES.light],
    langs: ["text"],
    preferredHighlighter: "shiki-js",
  }).catch((error: unknown) => console.error("Couldn't start the highlighter", error));
  pool = getOrCreateWorkerPoolSingleton({
    poolOptions: {
      workerFactory: () =>
        new Worker(new URL("@pierre/diffs/worker/worker.js", import.meta.url), { type: "module" }),
      poolSize: WORKERS,
      totalASTLRUCacheSize: CACHED_DIFFS,
    },
    // Oniguruma, as in VS Code, rather than Shiki's JavaScript engine: about twice as fast on a
    // diff of thousands of lines (1.5s against 3s for 6,000), and no slower on small ones. With
    // each token's place in its line, which editing a file needs: without, starting to edit one
    // highlights it all again on the main thread (1.9s for 2,500 lines, the window frozen), for
    // about 15% longer highlighting here (0.69s against 0.59s for 4,500 lines).
    highlighterOptions: {
      theme: THEMES,
      preferredHighlighter: "shiki-wasm",
      useTokenTransformer: true,
    },
  });
  return pool;
}

/**
 * How long a diff waits for its highlighting before it's shown without, until it's highlighted:
 * long enough for most, which then show highlighted at once rather than flicker from plain text,
 * short enough not to hold up one of thousands of lines, which takes a second or more.
 */
const HIGHLIGHT_WAIT_MS = 300;

/**
 * How long expanding the lines around the changes waits for the whole file to be highlighted. The
 * viewer gives it 300ms by itself, then shows the whole diff as plain text until it's done, which
 * flickers for a file of a thousand lines or so.
 */
const EXPAND_HIGHLIGHT_WAIT_MS = 1500;

/**
 * One file's patch, as git gave it, with only the lines on screen rendered. Only the changes and
 * the lines around them are in the patch; the rest of the file is read with `loadFile`, by the
 * object names on the patch's `index` line, once the lines between are asked for.
 *
 * Given another patch, it keeps the last one on show until the new one is highlighted (see
 * `HIGHLIGHT_WAIT_MS`), then shows it in its place and calls `onShown`. Another file's starts at
 * its top, with none of its lines shown around the changes; a new patch of the same file, which
 * uncommitted changes get whenever it's saved, takes the place of the last where it's scrolled to,
 * with the same lines shown around its changes. What it says of loading the whole file is of the
 * patch on show.
 *
 * With `staging`, the lines of an uncommitted file's patch can be picked to stage or unstage: by
 * selecting them, with the mouse or with the arrow keys (Shift to extend), or a hunk at a time,
 * with its button or `[` and `]`. The button in the gutter, by the changed line the pointer is on
 * or the last one selected, or Space stages them; Esc clears the selection. Once the keyboard
 * staged some, the selection moves to the change that's in their place.
 */
export default function PatchViewer(props: {
  patch: string;
  /** Names the file the patch is of: a new patch with the same name is of the same file. */
  fileKey: string;
  /**
   * Names the patch in the highlighting cache, e.g. by its commit and path: another patch, even of
   * the same file, needs another name, or it's shown with the last one's highlighting.
   */
  cacheKey: string;
  diffStyle: DiffStyle;
  /** Reads one side of the file in full, by the object name the patch has for it. */
  loadFile: (side: Side, oid: string) => Promise<LoadedFile>;
  /**
   * Edits the new side of the file in place, once it's been read whole; changing to false keeps
   * the edits as they are. Patches given meanwhile are held, as the editor would take them for its
   * text, and the last one is shown once editing stops.
   */
  editing?: boolean;
  /** Editing started (see `EditSession`), or stopped. */
  onEditing?: (session: EditSession | undefined) => void;
  /** The file was edited to `text`. */
  onEdit?: (text: string) => void;
  /** Why editing couldn't start, e.g. as the file changed again meanwhile. */
  onEditFailed?: (message: string) => void;
  /** Stages or unstages lines picked in the patch on show; not while it's edited. */
  staging?: LineStaging;
  /** The last patch given is on show. */
  onShown: () => void;
  /** Whether the whole file is being loaded, to show more of it. */
  onLoadingFiles: (loading: boolean) => void;
  /** Why the whole file couldn't be loaded, e.g. it's too large; `undefined` if it could. */
  onFilesError: (message: string | undefined) => void;
}) {
  const { theme } = useTheme();
  const next = createMemo(
    () => ({
      diff: parsed(props.patch, props.cacheKey),
      fileKey: props.fileKey,
      patch: props.patch,
    }),
    undefined,
    { equals: (a, b) => a.diff === b.diff && a.fileKey === b.fileKey },
  );
  let scroller: HTMLDivElement | undefined;
  let content: HTMLDivElement | undefined;
  let instance: ViewerFileDiff | undefined;
  /** The diff on show, once there's one, and the file it's of. */
  let shown: FileDiffMetadata | undefined;
  let shownFileKey: string | undefined;
  /** Loads of the whole file under way, by the diff they're for. */
  const loads = new Map<FileDiffMetadata, number>();
  /** Why the whole file couldn't be loaded, by the diff it's of: it's no use trying again. */
  const failures = new Map<FileDiffMetadata, string>();
  /** What picking lines to stage adds to the options, once the viewer's set up (see `setUp`). */
  let picking: (() => FileDiffOptions<HunkButton, undefined>) | undefined;
  /** Whether lines can be picked to stage: only of a patch that isn't being edited. */
  const pickable = () => props.staging !== undefined && !props.editing;
  /** The diffs this view had highlighted, by their cache key (see `dropLongHighlighting`). */
  const highlightedHere = new Map<string, FileDiffMetadata>();
  /** `highlighted`, remembering the diff as one this view had highlighted. */
  const highlight = (diff: FileDiffMetadata, ms: number) => {
    if (diff.cacheKey) highlightedHere.set(diff.cacheKey, diff);
    return highlighted(diff, ms);
  };
  /** `loadFiles`, remembering the diff filled in with the files as one this view highlighted. */
  const loadWhole = async (diff: FileDiffMetadata) => {
    const loaded = await loadFiles(diff, props.loadFile);
    if (loaded.hydrated.cacheKey) highlightedHere.set(loaded.hydrated.cacheKey, loaded.hydrated);
    return loaded;
  };
  /**
   * Drops the long diffs this view had highlighted from the workers' manager's cache, once the
   * view closes: the whole files read to show more lines or to edit them, the edits, and long
   * patches, which it would otherwise keep for as long as the app runs (see `CACHED_DIFFS`).
   */
  function dropLongHighlighting() {
    for (const [key, diff] of highlightedHere) {
      if (diff.additionLines.length + diff.deletionLines.length >= KEPT_LINES) {
        workerPool().evictDiffFromCache(key);
      }
    }
    highlightedHere.clear();
  }

  const options = (diffStyle: DiffStyle): FileDiffOptions<HunkButton, undefined> => ({
    theme: THEMES,
    themeType: theme(),
    diffStyle,
    // The view's own header names the file.
    disableFileHeader: true,
    unsafeCSS: APP_CSS,
    onPostRender: focusableExpandButtons,
    // Without the buttons to show more, once the whole file couldn't be loaded.
    hunkSeparators: shown && failures.has(shown) ? "simple" : "line-info",
    onEditChange: (event) => props.onEdit?.(event.file.contents),
    // The diff the library leaves the edits in is highlighted on the main thread, so it's not
    // kept: the viewer shows the edits in one of its own once editing stops (see `stopEditing`).
    onEditComplete: () => "reject",
    loadDiffFiles: async (diff) => {
      loads.set(diff, (loads.get(diff) ?? 0) + 1);
      report();
      try {
        return (await loadWhole(diff)).files;
      } catch (error) {
        failures.set(diff, error instanceof Error ? error.message : String(error));
        if (diff === shown) rerender();
        throw error;
      } finally {
        const left = loads.get(diff)! - 1;
        if (left > 0) loads.set(diff, left);
        else loads.delete(diff);
        report();
      }
    },
    ...picking?.(),
  });

  /** Says how loading the whole file on show is going. */
  function report() {
    props.onLoadingFiles(shown !== undefined && loads.has(shown));
    props.onFilesError(shown && failures.get(shown));
  }

  /** Renders the diff on show again, with options that may have changed. */
  function rerender() {
    if (!instance || !shown || !content) return;
    instance.setOptions(options(props.diffStyle));
    instance.render({ fileDiff: shown, containerWrapper: content, forceRender: true });
  }

  // Set up once on the page, as the virtualizer measures and observes its scroll container from
  // then on: inside a `Suspense` boundary, it can mount detached (see `useConnected`).
  const connected = useConnected(() => scroller);
  createEffect(() => {
    if (!connected() || !scroller || !content) return;
    untrack(() => setUp(scroller!, content!));
  });

  function setUp(root: HTMLDivElement, wrapper: HTMLDivElement) {
    // Picking lines to stage, in the patch on show.
    /** The changed lines of the diff on show, and the rows selected, if any. */
    let lines: ChangedLine[] = [];
    const [selection, setSelection] = createSignal<SelectedLineRange | null>(null);
    /** The changed lines selected. */
    const [picked, setPicked] = createSignal<ChangedLine[]>([]);
    /** Whether the selection is of whole hunks, gone through with `[` and `]`, not by line. */
    let byHunk = false;
    /**
     * Whether the diff on show is the patch's, which lines are picked from: not while it's edited,
     * nor the edits left on show after, until the next patch is.
     */
    let live = false;
    /**
     * Where the keyboard staged lines from, by the line numbers of the side the patch leaves as it
     * is (the working tree's, staging; HEAD's, unstaging): the change found there in the next patch
     * on show is selected (see `show`), or its hunk, if they were picked a hunk at a time; if they
     * couldn't be staged, the same lines are, in the patch refetched.
     */
    let stagedAt: { side: "old" | "new"; position: number; hunks: boolean } | undefined;
    /** The row a line is on in the diff on show, unified or side by side. */
    const rowOf: RowOf = (lineNumber, side) =>
      diffs.getLineIndex(lineNumber, side)?.[props.diffStyle === "split" ? 1 : 0];

    const select = (range: SelectedLineRange | null) => {
      setSelection(range);
      byHunk = false;
      setPicked(range ? selectedLines(lines, range, rowOf) : []);
    };

    /** Selects the changes of `line`'s hunk, to go through them a hunk at a time. */
    const selectHunk = (line: ChangedLine) => {
      const hunk = changedRows(lines, rowOf).filter((row) => row.line.hunk === line.hunk);
      diffs.setSelectedLines(rangeOf(hunk[0]!.line, hunk.at(-1)!.line));
      byHunk = true;
      return hunk;
    };

    /**
     * Whether the patch on show is the one lines are picked from: not one being edited, nor the
     * last file's, while the next one's is on its way, which `staging` isn't of.
     */
    const current = () => live && shownPatch?.fileKey === props.fileKey;

    /** Stages `changed`, of the patch on show, if lines can be staged now. */
    const stage = async (changed: ChangedLine[], byKeyboard: boolean) => {
      const staging = props.staging;
      if (!staging || staging.busy || !current() || changed.length === 0) return;
      const side: "new" | "old" = staging.action === "stage" ? "new" : "old";
      const at = byKeyboard
        ? { side, position: Math.min(...changed.map((line) => line[side])), hunks: byHunk }
        : undefined;
      stagedAt = at;
      try {
        await staging.onStage(shownPatch!.patch, toSelection(changed));
      } catch {
        // Not staged, e.g. as the file changed since: the patch refetched is selected in the same
        // place, if another is on its way, and the selection stays where it is otherwise.
        if (stagedAt === at && next() === shownPatch) stagedAt = undefined;
      }
    };

    /**
     * The changed line the pointer is on, if any, which the gutter's button stages without a
     * selection. Forgotten once another patch is on show, where it may be another line, until the
     * pointer moves.
     */
    const [hovered, setHovered] = createSignal<ChangedLine>();
    /** What the gutter's button stages: the lines selected, or else the line the pointer is on. */
    const toStage = () =>
      selection() ? picked() : [hovered()].filter((line) => line !== undefined);

    const unstaging = () => props.staging?.action === "unstage";
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
      const label = props.staging?.action === "unstage" ? "Unstage hunk" : "Stage hunk";
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

    picking = () => {
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
    /** The hunks' buttons for `diff`, if lines can be picked from it. */
    const buttons = (diff: FileDiffMetadata) => (pickable() ? hunkButtons(diff) : []);

    const virtualizer = new Virtualizer();
    virtualizer.setup(root, wrapper);
    const diffs = new ViewerFileDiff(
      options(props.diffStyle),
      virtualizer,
      undefined,
      workerPool(),
    );
    instance = diffs;
    let disposed = false;
    /** The editor while editing, how to finish it, and the patch on show (to know when another is). */
    let editor: Editor<"file-diff", HunkButton> | undefined;
    let finishEditing: (() => void) | undefined;
    let editing = false;
    /** The whole file's diff the edits started from. */
    let editedFrom: FileDiffMetadata | undefined;
    let shownPatch: ReturnType<typeof next> | undefined;
    onCleanup(() => {
      disposed = true;
      instance = undefined;
      if (finishEditing) {
        const edited = diffs.nameEditedDiff(`${shownFileKey}:edited:${++editedDiffs}`);
        if (edited) highlightedHere.set(edited.cacheKey!, edited);
        finishEditing();
        // Taken off the page while editing: that's over.
        props.onEditing?.(undefined);
      }
      diffs.cleanUp();
      virtualizer.cleanUp();
      dropLongHighlighting();
    });

    const show = async (patch: ReturnType<typeof next>) => {
      // Held while editing: the editor would take it for its text.
      if (editing) return;
      const sameFile = shown !== undefined && shownFileKey === patch.fileKey;
      // The same file, with lines shown around its changes: the new patch is filled in with the
      // whole file too, to show the same ones.
      const kept = sameFile && !shown!.isPartial ? await keptExpansion(patch.diff) : undefined;
      const diff = kept?.diff ?? patch.diff;
      await highlight(diff, HIGHLIGHT_WAIT_MS);
      // Passed over for another patch meanwhile, or editing started.
      if (disposed || next() !== patch || editing) return;
      // Another file starts at its top.
      if (shown && !sameFile) root.scrollTop = 0;
      // The lines shown around the last one's changes are by hunk, which another's don't match.
      diffs.expanded = kept?.expanded ?? new Map();
      if (sameFile && shown!.cacheKey !== diff.cacheKey) forget(shown!);
      shown = diff;
      shownFileKey = patch.fileKey;
      shownPatch = patch;
      live = true;
      // The lines selected are of the last patch: their numbers may be others' in this one.
      diffs.setSelectedLines(null);
      // A hunk's button that had the focus goes with its patch: the view keeps it.
      const focused = root.contains(document.activeElement);
      diffs.setOptions(options(props.diffStyle));
      diffs.render({ fileDiff: diff, containerWrapper: wrapper, lineAnnotations: buttons(diff) });
      if (focused && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
      lines = changedLines(diff);
      setHovered(undefined);
      reselect(sameFile);
      report();
      props.onShown();
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
      else diffs.setSelectedLines(rangeOf(target.line));
      reveal(target.line);
    };

    /**
     * `diff` filled in with the whole file, and the lines shown around the changes on show, moved
     * to its own; `undefined` if it can't be, e.g. as the file changed again since.
     */
    const keptExpansion = async (diff: FileDiffMetadata) => {
      const from = shown!;
      const expanded = diffs.expanded;
      try {
        const { hydrated } = await loadWhole(diff);
        return { diff: hydrated, expanded: carriedExpansion(from, expanded, hydrated) };
      } catch {
        return undefined;
      }
    };

    /**
     * Starts editing the file on show, once it's whole: a patch's diff is filled in with both
     * files, or for an added file, built from it. The lines shown around the changes stay, as the
     * hunks are the same.
     */
    const startEditing = async () => {
      const from = shown;
      if (!from || editing) return;
      editing = true;
      live = false;
      try {
        const [whole, { Editor }] = await Promise.all([
          wholeFile(from).then(async (file) => {
            // Highlighted in the workers first, which the editor starts from: it highlights the
            // whole file again on the main thread otherwise (1.6s for 2,500 lines), e.g. right
            // after editing stopped, while the edits are highlighted.
            await highlight(file.diff, EXPAND_HIGHLIGHT_WAIT_MS);
            return file;
          }),
          loadEditor(),
        ]);
        if (disposed || !editing || shown !== from) return;
        shown = whole.diff;
        editedFrom = whole.diff;
        diffs.render({ fileDiff: whole.diff, containerWrapper: wrapper, forceRender: true });
        // Without an `editStateKey`, which would keep the undo history for when the file's edited
        // again: picking it up has the library highlight the whole file again on the main thread
        // (1.6s for 2,500 lines, the window frozen), rather than start from the workers'.
        const opened = new Editor<"file-diff", HunkButton>("file-diff", {});
        editor = opened;
        finishEditing = opened.edit(diffs);
        opened.focus();
        props.onEditing?.({
          version: whole.version,
          text: opened.getText(),
          discard: () => void stopEditing(true),
          hasSelection: () => {
            const selections = opened.getViewState().selections ?? [];
            const [first] = selections;
            if (selections.length !== 1 || !first) return selections.length > 1;
            return (
              first.start.line !== first.end.line || first.start.character !== first.end.character
            );
          },
        });
      } catch (error) {
        if (disposed || !editing) return;
        editing = false;
        props.onEditFailed?.(error instanceof Error ? error.message : String(error));
      }
    };

    /**
     * Stops editing, keeping the edits on show unless they're dropped, then shows the last patch.
     * The edits are shown in a diff of the viewer's own, highlighted in the workers first, in the
     * editor's place: the library's would be highlighted on the main thread, once as editing stops
     * and again as it starts over (1.6s each for 2,500 lines, the window frozen).
     */
    const stopEditing = async (discard: boolean) => {
      if (!editing) return;
      const opened = editor;
      // No more typing: what's saved is what's left.
      opened?.blur();
      const left = !discard && opened ? await editedDiff(opened) : undefined;
      if (!editing || editor !== opened) return;
      editing = false;
      editor = undefined;
      // What the library highlights again once editing stops: in the workers, as it's named.
      const session = diffs.nameEditedDiff(`${shownFileKey}:session:${++editedDiffs}`);
      if (session) highlightedHere.set(session.cacheKey!, session);
      if (discard) opened?.cleanUp("discard");
      else finishEditing?.();
      finishEditing = undefined;
      if (left && session) {
        diffs.expanded = carriedExpansion(session, diffs.expanded, left);
        shown = left;
        diffs.render({ fileDiff: left, containerWrapper: wrapper });
      }
      props.onEditing?.(undefined);
      if (next() !== shownPatch) void show(next());
    };

    /** The edits as `opened` has them, in a whole file's diff, highlighted. */
    const editedDiff = async (opened: Editor<"file-diff", HunkButton>) => {
      const from = editedFrom!;
      const oldFile =
        from.type === "new"
          ? null
          : { name: from.prevName ?? from.name, contents: from.deletionLines.join("") };
      // Typing can go on while it's highlighted: then it's the latest text's that's shown.
      for (let tries = 0; ; tries++) {
        const text = opened.getText();
        const diff = parseDiffFromFile(oldFile, { name: from.name, contents: text });
        diff.cacheKey = `${shownFileKey}:edited:${++editedDiffs}`;
        // oxlint-disable-next-line no-await-in-loop -- the text it's of may have changed since.
        await highlight(diff, EXPAND_HIGHLIGHT_WAIT_MS);
        if (opened.getText() === text || tries === 2) return diff;
      }
    };

    /** `diff` with the whole file, and the version of the working tree's it's of. */
    const wholeFile = async (diff: FileDiffMetadata) => {
      if (diff.type === "new") {
        const read = await readNewSide(diff);
        const contents = fitToLines(diff.additionLines, read.contents);
        const whole = parseDiffFromFile(null, {
          name: diff.name,
          contents,
          cacheKey: diff.cacheKey && `${diff.cacheKey}:whole`,
        });
        return { diff: whole, version: read.version, text: contents };
      }
      if (diff.isPartial) {
        const { hydrated, version } = await loadWhole(diff);
        if (!version) throw new Error("This file can't be edited.");
        return { diff: hydrated, version, text: hydrated.additionLines.join("") };
      }
      // Whole already, as more of it was shown, or it was edited: still the file on disk?
      const read = await readNewSide(diff);
      return { diff, version: read.version, text: fitToLines(diff.additionLines, read.contents) };
    };

    /** The working tree's side of `diff`, with its version. */
    const readNewSide = async (diff: FileDiffMetadata) => {
      const read = await props.loadFile("new", diff.newObjectId ?? "");
      if (typeof read === "string") throw new Error("This file can't be edited.");
      return read;
    };

    createEffect(
      on(
        () => props.editing,
        (edit) => void (edit ? startEditing() : stopEditing(false)),
        { defer: true },
      ),
    );
    createEffect(on(next, (patch) => void show(patch)));
    createEffect(on(() => props.diffStyle, rerender, { defer: true }));
    // Lines can be picked, or no longer (as editing starts, say): the hunks' buttons, and selecting
    // lines, come and go with that, on the patch on show.
    createEffect(
      on(
        pickable,
        (can) => {
          // Another file's patch is on its way, with the buttons it has.
          if (!shown || !current()) return;
          if (!can) diffs.setSelectedLines(null);
          diffs.setLineAnnotations(buttons(shown));
          rerender();
        },
        { defer: true },
      ),
    );

    /** Scrolls the view, if it has to, to show `line`'s row. */
    const reveal = (line: ChangedLine) => {
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
      diffs.setSelectedLines(
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
        const firsts = rows.filter(
          ({ line }, i) => i === 0 || rows[i - 1]!.line.hunk !== line.hunk,
        );
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
      if (!pickable() || !current() || event.defaultPrevented) return;
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
          diffs.setSelectedLines(null);
          break;
        default:
          return;
      }
      event.preventDefault();
    };
    root.addEventListener("keydown", onKeyDown);
    onCleanup(() => root.removeEventListener("keydown", onKeyDown));
    createEffect(on(theme, (type) => diffs.setThemeType(type), { defer: true }));
  }

  return (
    <div
      ref={(el) => (scroller = el)}
      // Focusable, to scroll it with the keyboard, and pick lines to stage with it.
      tabIndex={0}
      aria-label="Changed lines"
      aria-keyshortcuts={
        pickable() ? "ArrowUp ArrowDown Shift+ArrowUp Shift+ArrowDown [ ] Space" : undefined
      }
      data-busy={props.staging?.busy || undefined}
      class="group h-full min-h-0 overflow-auto bg-bg outline-none [--diffs-font-size:12px] [--diffs-line-height:20px] focus-visible:shadow-[inset_0_0_0_1px_var(--primary)]"
    >
      {/* Never empty: before its first render, a diff without height at the top of the view is
          taken by the virtualizer for one above it, and kept in place by its bottom edge, which
          scrolls the view to the end of the file once its lines are in. */}
      <div ref={(el) => (content = el)} class="[&>*]:min-h-px" />
    </div>
  );
}

/** The library's virtualized diff, with some of what it keeps to itself in reach. */
class ViewerFileDiff extends VirtualizedFileDiff<HunkButton, undefined> {
  /**
   * The lines shown around each hunk: they're kept by hunk across diffs, so another file's would
   * show the last one's (see `show`).
   */
  get expanded(): ReadonlyMap<number, HunkExpansionRegion> {
    return this.hunksRenderer.getExpandedHunksMap();
  }

  set expanded(expanded: Map<number, HunkExpansionRegion>) {
    this.hunksRenderer.setExpandedHunksMap(expanded);
  }

  /**
   * Names the diff being edited in the highlighting cache, which the library leaves unnamed: once
   * editing stops, a diff with edits is highlighted again, in the workers only if it has a name,
   * on the main thread otherwise (1.7s for 2,500 lines, the window frozen). Returns the diff, if
   * one's being edited, named as it was if it already had a name.
   */
  nameEditedDiff(cacheKey: string): FileDiffMetadata | undefined {
    const diff = this.getLatestDiff();
    if (diff && diff.cacheKey == null) diff.cacheKey = cacheKey;
    return diff;
  }
}

/** Tells apart the diffs left by editing, in the highlighting cache. */
let editedDiffs = 0;

/**
 * Drops a patch of a file that's been replaced by a newer one from the highlighting cache, which
 * has room for `CACHED_DIFFS`: an uncommitted file can be saved many times while it's on show.
 */
function forget(diff: FileDiffMetadata) {
  if (!diff.cacheKey) return;
  workerPool().evictDiffFromCache(diff.cacheKey);
  const partial = diff.cacheKey.replace(/:hydrated$/, "");
  if (partial !== diff.cacheKey) workerPool().evictDiffFromCache(partial);
}

/**
 * Makes the buttons that show more lines reachable with the keyboard, after each render, as the
 * library draws them as `div`s that take a click: Enter or Space clicks the one with the focus.
 */
function focusableExpandButtons(container: HTMLElement, _: unknown, phase: PostRenderPhase) {
  const root = container.shadowRoot;
  if (!root || phase === "unmount") return;
  for (const button of root.querySelectorAll<HTMLElement>("[data-expand-button]:not([tabindex])")) {
    button.tabIndex = 0;
    button.setAttribute("aria-label", "Show unmodified lines");
  }
  if (phase === "mount") root.addEventListener("keydown", clickExpandButton);
}

function clickExpandButton(event: Event) {
  const { key, target } = event as KeyboardEvent;
  if (key !== "Enter" && key !== " ") return;
  if (!(target instanceof HTMLElement) || !target.hasAttribute("data-expand-button")) return;
  event.preventDefault();
  target.click();
}

/** Resolves once `diff` is highlighted, or `ms` later; never rejects. */
function highlighted(diff: FileDiffMetadata, ms: number): Promise<void> {
  return Promise.race([
    // Shown as plain text if it can't be highlighted.
    workerPool()
      .primeDiffHighlightCache(diff)
      .catch(() => undefined),
    new Promise<void>((resolve) => setTimeout(resolve, ms)),
  ]);
}

/** Patches parsed lately, by their cache key, so one prepared ahead isn't parsed again. */
const parsedPatches = new Map<string, { patch: string; diff: FileDiffMetadata }>();
const PARSED_PATCHES_KEPT = 16;

/** `parsePatch`, of a patch that may have been parsed lately. */
function parsed(patch: string, cacheKey: string): FileDiffMetadata {
  const kept = parsedPatches.get(cacheKey);
  if (kept?.patch === patch) return kept.diff;
  const diff = parsePatch(patch, cacheKey);
  parsedPatches.delete(cacheKey);
  parsedPatches.set(cacheKey, { patch, diff });
  // The oldest go first: a map keeps the order things were put in.
  for (const key of parsedPatches.keys()) {
    if (parsedPatches.size <= PARSED_PATCHES_KEPT) break;
    parsedPatches.delete(key);
  }
  return diff;
}

/**
 * Parses and highlights a patch ahead of showing it, with the same `cacheKey`, so it shows at once.
 * Rejects if it can't be parsed.
 */
export async function preparePatch(patch: string, cacheKey: string): Promise<void> {
  await workerPool().primeDiffHighlightCache(parsed(patch, cacheKey));
}

/** The one file in `patch`. */
export function parsePatch(patch: string, cacheKey: string): FileDiffMetadata {
  const [file, other] = parsePatchFiles(patch, cacheKey)[0]?.files ?? [];
  if (!file) throw new Error("The patch has no file in it.");
  // A file that changed type, e.g. from a symbolic link, is a deletion and an addition to git,
  // each with the whole of its side: shown as one change, from the one's contents to the other's.
  if (file.type === "deleted" && other?.type === "new") {
    return parseDiffFromFile(
      { name: file.name, contents: file.deletionLines.join(""), cacheKey: `${cacheKey}:old` },
      { name: other.name, contents: other.additionLines.join(""), cacheKey: `${cacheKey}:new` },
    );
  }
  return file;
}

/**
 * Both sides of `diff` in full, read by the object names on its `index` line, and a copy of `diff`
 * filled in with them. Rejects if they aren't the files the patch is of, as a file in the working
 * tree can have changed since.
 */
async function loadFiles(
  diff: FileDiffMetadata,
  loadFile: (side: Side, oid: string) => Promise<LoadedFile>,
): Promise<{ files: FileDiffLoadedFiles; hydrated: FileDiffMetadata; version?: string }> {
  const { prevObjectId, newObjectId } = diff;
  if (!prevObjectId || !newObjectId) throw new Error("The patch doesn't name its files.");
  const [old, read] = await Promise.all([
    loadFile("old", prevObjectId),
    loadFile("new", newObjectId),
  ]);
  const before = typeof old === "string" ? old : old.contents;
  const after = typeof read === "string" ? read : read.contents;
  // Named in the highlighting cache by the patch's object names: the working tree's side is by
  // what git would store it as, so another version of the file has another name too.
  const files = {
    oldFile: {
      name: diff.prevName ?? diff.name,
      contents: fitToPatch(diff, "old", before),
      cacheKey: prevObjectId,
    },
    newFile: { name: diff.name, contents: fitToPatch(diff, "new", after), cacheKey: newObjectId },
  };
  const hydrated = hydratePartialDiff("clone", diff, files);
  if (!hasMatchingEnds(hydrated)) {
    throw new Error("The file has changed since its changes were loaded. Try again in a moment.");
  }
  // Highlighted before they're handed over (see `EXPAND_HIGHLIGHT_WAIT_MS`): the copy, filled in
  // as the viewer will, has the same key in the highlighting cache.
  await highlighted(hydrated, EXPAND_HIGHLIGHT_WAIT_MS);
  return { files, hydrated, version: typeof read === "string" ? undefined : read.version };
}
