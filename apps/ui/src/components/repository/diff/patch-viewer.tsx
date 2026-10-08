// The diff viewer, from @pierre/diffs. Loaded on its own (see `FileDiffView`), as it brings Shiki.
import { Virtualizer, type FileDiffMetadata, type FileDiffOptions } from "@pierre/diffs";
import { createEffect, createMemo, onCleanup, on, untrack } from "solid-js";

import { useConnected } from "@/hooks/connected";
import type { DiffStyle } from "@/hooks/diff-style";
import { useTheme } from "@/hooks/theme";
import { errorMessage } from "@/lib/errors";

import { createLinePicker, type LinePicker, type LineStaging } from "./line-picker";
import type { HunkButton } from "./line-staging";
import { carriedExpansion, type Side } from "./patch-files";
import { createViewerEditing, type EditSession } from "./viewer-editing";
import {
  APP_CSS,
  focusableExpandButtons,
  HIGHLIGHT_WAIT_MS,
  parsed,
  THEMES,
  ViewerFileDiff,
  ViewHighlights,
  workerPool,
  type LoadedFile,
} from "./viewer-runtime";

export type { LineStaging } from "./line-picker";
export type { EditSession } from "./viewer-editing";
export { parsePatch, preparePatch, type LoadedFile } from "./viewer-runtime";

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
  /** The patch to show, parsed: the last one given, which the one on show makes way for. */
  const nextPatch = createMemo(
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
  let shownDiff: FileDiffMetadata | undefined;
  let shownFileKey: string | undefined;
  /** Loads of the whole file under way, by the diff they're for. */
  const loads = new Map<FileDiffMetadata, number>();
  /** Why the whole file couldn't be loaded, by the diff it's of: it's no use trying again. */
  const failures = new Map<FileDiffMetadata, string>();
  /** Picking lines to stage, which adds to the options, once the viewer's set up (see `setUp`). */
  let picker: LinePicker | undefined;
  /** Whether lines can be picked to stage: only of a patch that isn't being edited. */
  const pickable = () => props.staging !== undefined && !props.editing;
  /** The diffs this view had highlighted, to drop the long ones once it closes. */
  const highlights = new ViewHighlights();

  const options = (diffStyle: DiffStyle): FileDiffOptions<HunkButton, undefined> => ({
    theme: THEMES,
    themeType: theme(),
    diffStyle,
    // The view's own header names the file.
    disableFileHeader: true,
    unsafeCSS: APP_CSS,
    onPostRender: focusableExpandButtons,
    // Without the buttons to show more, once the whole file couldn't be loaded.
    hunkSeparators: shownDiff && failures.has(shownDiff) ? "simple" : "line-info",
    onEditChange: (event) => props.onEdit?.(event.file.contents),
    // The diff the library leaves the edits in is highlighted on the main thread, so it's not
    // kept: the viewer shows the edits in one of its own once editing stops (see
    // `createViewerEditing`'s `stop`).
    onEditComplete: () => "reject",
    loadDiffFiles: async (diff) => {
      loads.set(diff, (loads.get(diff) ?? 0) + 1);
      report();
      try {
        return (await highlights.loadWhole(diff, props.loadFile)).files;
      } catch (error) {
        failures.set(diff, errorMessage(error));
        if (diff === shownDiff) rerender();
        throw error;
      } finally {
        const left = loads.get(diff)! - 1;
        if (left > 0) loads.set(diff, left);
        else loads.delete(diff);
        report();
      }
    },
    ...picker?.options(),
  });

  /** Says how loading the whole file on show is going. */
  function report() {
    props.onLoadingFiles(shownDiff !== undefined && loads.has(shownDiff));
    props.onFilesError(shownDiff && failures.get(shownDiff));
  }

  /** Renders the diff on show again, with options that may have changed. */
  function rerender() {
    if (!instance || !shownDiff || !content) return;
    instance.setOptions(options(props.diffStyle));
    instance.render({ fileDiff: shownDiff, containerWrapper: content, forceRender: true });
  }

  // Set up once on the page, as the virtualizer measures and observes its scroll container from
  // then on: inside a `Suspense` boundary, it can mount detached (see `useConnected`).
  const connected = useConnected(() => scroller);
  createEffect(() => {
    if (!connected() || !scroller || !content) return;
    untrack(() => setUp(scroller!, content!));
  });

  function setUp(root: HTMLDivElement, wrapper: HTMLDivElement) {
    /**
     * Whether the diff on show is the patch's, which lines are picked from: not while it's edited,
     * nor the edits left on show after, until the next patch is.
     */
    let unedited = false;
    /** The patch on show, of `nextPatch`'s, to know when another is. */
    let patchOnShow: ReturnType<typeof nextPatch> | undefined;
    /**
     * Whether the patch on show is the one lines are picked from: not one being edited, nor the
     * last file's, while the next one's is on its way, which `staging` isn't of.
     */
    const isLive = () => unedited && patchOnShow?.fileKey === props.fileKey;

    // Before the diff, whose options it adds to.
    const picking = createLinePicker({
      root,
      diffs: () => diffs,
      diffStyle: () => props.diffStyle,
      staging: () => props.staging,
      pickable,
      isLive,
      shownPatch: () => patchOnShow?.patch,
      awaitingPatch: () => nextPatch() !== patchOnShow,
    });
    picker = picking;
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
    const editing = createViewerEditing({
      diffs,
      wrapper,
      highlights,
      shownDiff: () => shownDiff,
      setShownDiff: (diff) => (shownDiff = diff),
      shownFileKey: () => shownFileKey,
      disposed: () => disposed,
      loadFile: (side, oid) => props.loadFile(side, oid),
      onEditing: (session) => props.onEditing?.(session),
      onEditFailed: (message) => props.onEditFailed?.(message),
      onStarted: () => (unedited = false),
      onStopped: () => {
        if (nextPatch() !== patchOnShow) void show(nextPatch());
      },
    });
    onCleanup(() => {
      disposed = true;
      instance = undefined;
      editing.dispose();
      diffs.cleanUp();
      virtualizer.cleanUp();
      highlights.drop();
    });

    const show = async (patch: ReturnType<typeof nextPatch>) => {
      // Held while editing: the editor would take it for its text.
      if (editing.active()) return;
      const sameFile = shownDiff !== undefined && shownFileKey === patch.fileKey;
      // The same file, with lines shown around its changes: the new patch is filled in with the
      // whole file too, to show the same ones. Not an added or deleted file's, whose patch has
      // all its lines, and no other side to read (git names it with zeros): as once it's edited.
      const whole = patch.diff.type === "new" || patch.diff.type === "deleted";
      const kept =
        sameFile && !shownDiff!.isPartial && !whole ? await keptExpansion(patch.diff) : undefined;
      const diff = kept?.diff ?? patch.diff;
      await highlights.highlight(diff, HIGHLIGHT_WAIT_MS);
      // Passed over for another patch meanwhile, or editing started.
      if (disposed || nextPatch() !== patch || editing.active()) return;
      // Another file starts at its top.
      if (shownDiff && !sameFile) root.scrollTop = 0;
      // The lines shown around the last one's changes are by hunk, which another's don't match.
      diffs.expanded = kept?.expanded ?? new Map();
      if (sameFile && shownDiff!.cacheKey !== diff.cacheKey) highlights.forget(shownDiff!);
      shownDiff = diff;
      shownFileKey = patch.fileKey;
      patchOnShow = patch;
      unedited = true;
      // The lines selected are of the last patch: their numbers may be others' in this one.
      diffs.setSelectedLines(null);
      // A hunk's button that had the focus goes with its patch: the view keeps it.
      const focused = root.contains(document.activeElement);
      diffs.setOptions(options(props.diffStyle));
      diffs.render({
        fileDiff: diff,
        containerWrapper: wrapper,
        lineAnnotations: picking.buttons(diff),
      });
      if (focused && !root.contains(document.activeElement)) root.focus({ preventScroll: true });
      picking.shown(diff, sameFile);
      report();
      props.onShown();
    };

    /**
     * `diff` filled in with the whole file, and the lines shown around the changes on show, moved
     * to its own; `undefined` if it can't be, e.g. as the file changed again since.
     */
    const keptExpansion = async (diff: FileDiffMetadata) => {
      const from = shownDiff!;
      const expanded = diffs.expanded;
      try {
        const { hydrated } = await highlights.loadWhole(diff, props.loadFile);
        return { diff: hydrated, expanded: carriedExpansion(from, expanded, hydrated) };
      } catch {
        return undefined;
      }
    };

    createEffect(
      on(
        () => props.editing,
        (edit) => void (edit ? editing.start() : editing.stop(false)),
        { defer: true },
      ),
    );
    createEffect(on(nextPatch, (patch) => void show(patch)));
    createEffect(on(() => props.diffStyle, rerender, { defer: true }));
    // Lines can be picked, or no longer (as editing starts, say): the hunks' buttons, and selecting
    // lines, come and go with that, on the patch on show.
    createEffect(
      on(
        pickable,
        (can) => {
          // Another file's patch is on its way, with the buttons it has.
          if (!shownDiff || !isLive()) return;
          if (!can) diffs.setSelectedLines(null);
          diffs.setLineAnnotations(picking.buttons(shownDiff));
          rerender();
        },
        { defer: true },
      ),
    );
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
