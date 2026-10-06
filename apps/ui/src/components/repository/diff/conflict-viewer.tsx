// The conflicts in a file, from @pierre/diffs. Loaded on its own (see `useConflictResolution`), as
// it brings Shiki.
import {
  hydratePartialDiff,
  UnresolvedFile,
  type FileDiffMetadata,
  type UnresolvedFileOptions,
} from "@pierre/diffs";
import { createEffect, on, onCleanup, untrack } from "solid-js";

import { useConnected } from "@/hooks/connected";
import { useTheme } from "@/hooks/theme";

import {
  markConflictRows,
  renderConflictRow,
  scrollToConflict,
  type Resolution,
} from "./conflict-actions";
import {
  conflictsCacheKey,
  conflictsLeft,
  CONTEXT_LINES,
  parseConflicts,
  readConflicts,
  wholeSides,
  type ConflictState,
} from "./conflict-diff";
import { createWholeFile } from "./conflict-whole-file";
import type { EditSession } from "./viewer-editing";
import {
  APP_CSS,
  EXPAND_HIGHLIGHT_WAIT_MS,
  focusableExpandButtons,
  HIGHLIGHT_WAIT_MS,
  THEMES,
  ViewHighlights,
  workerPool,
} from "./viewer-runtime";

export type { Resolution } from "./conflict-actions";

/** What the header does to the conflicts on show. */
export interface ConflictCommands {
  /** Scrolls to the next conflict left, or the previous one, which becomes the current one. */
  next: () => void;
  previous: () => void;
  /** Resolves the current conflict. */
  resolve: (resolution: Resolution) => void;
}

/** How resolving the file on show is going. */
export interface ConflictProgress {
  /** Names the file on show (see `fileKey`): another one's, until this one's is on show. */
  fileKey: string | undefined;
  /** How many conflicts are left. */
  left: number;
  /** Where the current one is among them, from 1; `undefined` without one. */
  current: number | undefined;
  /** Whether a conflict is being resolved: written to disk, and shown once it is. */
  saving: boolean;
  /** The version of the file on show on disk; `null` until it's known, e.g. after editing. */
  version: string | null;
  /** Why the file's conflicts aren't shown one by one, as its whole text is instead. */
  problem: string | undefined;
}

/** The file on disk, as last read. */
export interface DiskFile {
  contents: string;
  version: string;
}

/** The library's view of a conflicted file, with the lines it shows around the conflicts in reach. */
class ConflictFile extends UnresolvedFile<undefined> {
  /** Shows no more lines around the conflicts than it did at first, as for another file. */
  resetExpanded(): void {
    this.hunksRenderer.setExpandedHunksMap(new Map());
  }

  /** Shows `state`, keeping the lines shown around the conflicts unless `reset`. */
  show(state: ConflictState, wrapper: HTMLElement, reset: boolean): void {
    if (reset) this.resetExpanded();
    this.render({
      file: state.file,
      fileDiff: state.diff,
      actions: state.actions,
      markerRows: state.markerRows,
      containerWrapper: wrapper,
      forceRender: reset,
    });
  }
}

/** The file's text and what's shown of it: its conflicts, or the whole of it without. */
interface Shown {
  fileKey: string;
  contents: string;
  /** Its version on disk; `null` while that isn't known. */
  version: string | null;
  /** Its conflicts, if they're shown one by one. */
  state: ConflictState | undefined;
  problem: string | undefined;
}

/**
 * A conflicted file's conflicts, each with buttons to keep ours, theirs or both, and the lines
 * around them, more of which are shown when asked. Resolving one rewrites its region of the file,
 * which is written to disk (`save`) before it's shown, highlighted: in its place, so the view stays
 * where it is. A file without conflicts left, or whose markers can't be read as conflicts, is shown
 * whole instead (see `createWholeFile`).
 *
 * Given the file as it's read from disk again (`disk`), it shows it only if it's changed by
 * something else since, e.g. another editor: not again as it was written here, nor as it was read
 * before that. While it's edited (`editing`), the whole file is, markers and all, in place, and
 * what's read from disk is held until editing stops.
 */
export default function ConflictViewer(props: {
  /** Names the file: another name is another file, which starts at its top. */
  fileKey: string;
  path: string;
  /** The file as it was last read from disk; `undefined` until it's read. */
  disk: DiskFile | undefined;
  /** Writes `contents` over the file at `version`; resolves to its new version. */
  save: (contents: string, version: string) => Promise<string>;
  /** Edits the whole file in place; changing to false keeps the edits on show. */
  editing: boolean;
  onEditing: (session: EditSession | undefined) => void;
  onEdit: (text: string) => void;
  onEditFailed: (message: string) => void;
  onProgress: (progress: ConflictProgress) => void;
  onCommands: (commands: ConflictCommands | undefined) => void;
  /** Why the last conflict couldn't be resolved; `undefined` once one is. */
  onError: (message: string | undefined) => void;
  /** The file read last is on show. */
  onShown: () => void;
}) {
  const { theme } = useTheme();
  let scroller: HTMLDivElement | undefined;
  let conflictsWrapper: HTMLDivElement | undefined;
  let fileWrapper: HTMLDivElement | undefined;

  const connected = useConnected(() => scroller);
  createEffect(() => {
    if (!connected() || !scroller || !conflictsWrapper || !fileWrapper) return;
    untrack(() => setUp(scroller!, conflictsWrapper!, fileWrapper!));
  });

  function setUp(root: HTMLDivElement, wrapper: HTMLDivElement, wholeWrapper: HTMLDivElement) {
    const highlights = new ViewHighlights();
    /** The file on show. */
    let shown: Shown | undefined;
    /** The text each diff on show was of, to fill it in with the whole file's. */
    const texts = new WeakMap<FileDiffMetadata, string>();
    /** Versions of the file that were written over here: read from disk before that, and stale. */
    const superseded = new Set<string>();
    /** The conflict the header's buttons and keys are of, by its index. */
    let current: number | undefined;
    let saving = false;
    let editing = false;
    let disposed = false;

    /**
     * Whether a conflict can be resolved now: not while another's being saved, nor before the
     * file's version on disk is known, which the write is checked against (after editing, say).
     */
    const resolvable = () => !saving && !!shown?.version;

    const report = () => {
      const left = conflictsLeft(shown?.state);
      const at = current === undefined ? -1 : left.indexOf(current);
      props.onProgress({
        fileKey: shown?.fileKey,
        left: left.length,
        current: at === -1 ? undefined : at + 1,
        saving,
        version: shown?.version ?? null,
        problem: shown?.problem,
      });
      markConflictRows(wrapper, current, resolvable());
    };

    const options = (): UnresolvedFileOptions<undefined> => ({
      theme: THEMES,
      themeType: theme(),
      disableFileHeader: true,
      unsafeCSS: CONFLICT_CSS,
      onPostRender: focusableExpandButtons,
      hunkSeparators: "line-info",
      maxContextLines: CONTEXT_LINES,
      mergeConflictActionsType: (action) =>
        renderConflictRow(
          action,
          { current: action.conflictIndex === current, enabled: resolvable() },
          // By its index, on the text on show when it's clicked: the library keeps a row drawn
          // for an earlier text, with this closure, if its conflict is laid out the same.
          (resolution) => request(action.conflictIndex, resolution),
        ),
      // Told what the user chose, rather than resolving by itself: it's written to disk first.
      onMergeConflictAction: ({ conflict, resolution }) =>
        request(conflict.conflictIndex, resolution),
      // The whole of both sides, from the text the diff is of: to show more lines around the
      // conflicts, highlighted first, as `loadFiles` does for a patch.
      loadDiffFiles: async (diff) => {
        const text = texts.get(diff);
        if (text === undefined || !diff.cacheKey) throw new Error("The file has changed since.");
        const files = wholeSides(props.path, text, diff.cacheKey);
        await highlights.highlight(
          hydratePartialDiff("clone", diff, files),
          EXPAND_HIGHLIGHT_WAIT_MS,
        );
        return files;
      },
    });
    const conflicts = new ConflictFile(options(), workerPool());
    const whole = createWholeFile({
      root,
      conflicts: wrapper,
      wrapper: wholeWrapper,
      path: () => props.path,
      theme,
      disposed: () => disposed,
      onEdit: (text) => props.onEdit(text),
    });

    /**
     * Shows `contents`: its conflicts, or the whole of it. Another file starts at its top; the
     * same one changed elsewhere stays where it's scrolled to.
     */
    const showText = async (fileKey: string, contents: string, version: string | null) => {
      const other = shown?.fileKey !== fileKey;
      // Named by what's in it: a file that's back to what it was is shown as it was then.
      const cacheKey = conflictsCacheKey(fileKey, contents, version);
      const { state, problem } = readConflicts(props.path, contents, cacheKey);
      if (state) {
        await highlights.highlight(state.diff, HIGHLIGHT_WAIT_MS);
        if (disposed) return;
        whole.hide();
        texts.set(state.diff, contents);
        if (other) root.scrollTop = 0;
        // The lines shown around the conflicts are by hunk, which another text's don't match.
        conflicts.show(state, wrapper, true);
        const last = shown?.state?.diff;
        if (last && last !== state.diff) highlights.forget(last);
      } else {
        await whole.show(contents, cacheKey);
        if (disposed) return;
        if (other) root.scrollTop = 0;
      }
      shown = { fileKey, contents, version, state, problem };
      current = conflictsLeft(state)[0];
      report();
      // Only the file as it was read from disk: not edits whose version isn't known yet.
      if (version !== null) props.onShown();
    };

    // What's to be done, one thing at a time: the file as it was read again, and the conflicts
    // the user resolved, in turn. Editing holds the rest until it stops.
    let wanted: { fileKey: string; disk: DiskFile } | undefined;
    /** Conflicts to resolve, each by its index in the text that was on show when asked. */
    const requests: { state: ConflictState; index: number; resolution: Resolution }[] = [];
    let running = false;
    /** The work under way, which editing waits for. */
    let active: Promise<void> = Promise.resolve();

    const run = (): Promise<void> => {
      if (running) return active;
      running = true;
      active = work().finally(() => (running = false));
      return active;
    };
    const work = async () => {
      for (;;) {
        if (disposed || editing) return;
        const disk = wanted;
        wanted = undefined;
        const next = disk ? undefined : requests.shift();
        if (disk) {
          // oxlint-disable-next-line no-await-in-loop -- one at a time, in order.
          await sync(disk.fileKey, disk.disk);
        } else if (next) {
          // oxlint-disable-next-line no-await-in-loop -- each from the text the last one left.
          await resolveConflict(next.state, next.index, next.resolution);
        } else {
          return;
        }
      }
    };

    /** Shows the file as it was read from disk, if that's news. */
    const sync = async (fileKey: string, disk: DiskFile) => {
      if (shown?.fileKey !== fileKey) {
        superseded.clear();
        requests.length = 0;
        await showText(fileKey, disk.contents, disk.version);
        return;
      }
      if (disk.contents === shown.contents) {
        // Written here, or the same again: only its version is news.
        const known = shown.version !== null;
        shown.version = disk.version;
        report();
        if (!known) props.onShown();
        return;
      }
      if (superseded.has(disk.version)) return;
      // Changed elsewhere: what was asked of the last text isn't of this one.
      requests.length = 0;
      await showText(fileKey, disk.contents, disk.version);
    };

    /** Resolves the conflict `index` of the text on show, once what's asked before is done. */
    const request = (index: number, resolution: Resolution) => {
      const state = shown?.state;
      if (!state?.actions[index]) return;
      requests.push({ state, index, resolution });
      void run();
    };

    /**
     * Resolves the conflict `index` of `asked`, the text that was on show when it was asked, if
     * that's still the one on show: writes the text that leaves to disk, and shows it once it's
     * written and highlighted, in the last one's place.
     */
    const resolveConflict = async (asked: ConflictState, index: number, resolution: Resolution) => {
      const from = shown;
      if (!from?.state || from.state !== asked || from.version === null) return;
      const resolved = conflicts.resolveConflict(index, resolution, from.state.diff);
      if (!resolved) return;
      const state: ConflictState = {
        file: resolved.file,
        diff: resolved.fileDiff,
        actions: resolved.actions,
        markerRows: resolved.markerRows,
      };
      saving = true;
      report();
      try {
        const [version] = await Promise.all([
          props.save(resolved.file.contents, from.version),
          highlights.highlight(state.diff, HIGHLIGHT_WAIT_MS),
        ]);
        superseded.add(from.version);
        if (disposed) return;
        texts.set(state.diff, resolved.file.contents);
        // In the last one's place: the lines shown around the conflicts are kept, by hunk.
        conflicts.show(state, wrapper, false);
        highlights.forget(from.state.diff);
        shown = { ...from, contents: resolved.file.contents, version, state };
        // On to the next one, or the one before if it was the last.
        const left = conflictsLeft(state);
        current = left.find((other) => other > index) ?? left.at(-1);
        props.onError(undefined);
      } catch (error) {
        // Not shown after all: its highlighting isn't kept either.
        highlights.forget(state.diff);
        props.onError(error instanceof Error ? error.message : String(error));
      } finally {
        saving = false;
        report();
      }
    };

    /** Goes to the conflict after the current one, or before it, round to the other end. */
    const step = (by: 1 | -1) => {
      const left = conflictsLeft(shown?.state);
      const at = current === undefined ? -1 : left.indexOf(current);
      const next =
        at === -1
          ? by === 1
            ? left[0]
            : left.at(-1)
          : left[(at + by + left.length) % left.length];
      if (next === undefined) return;
      current = next;
      report();
      scrollToConflict(wrapper, next);
    };
    props.onCommands({
      next: () => step(1),
      previous: () => step(-1),
      resolve: (resolution) => {
        if (current !== undefined) request(current, resolution);
      },
    });

    /** Edits the whole file, at the current conflict, once what's under way is done. */
    const startEditing = async () => {
      if (editing) return;
      editing = true;
      await active;
      const from = shown;
      if (!from || from.version === null) {
        editing = false;
        props.onEditFailed("The file is still being saved. Try again in a moment.");
        return;
      }
      const action = current === undefined ? undefined : from.state?.actions[current];
      try {
        const edit = await whole.edit(
          from.contents,
          conflictsCacheKey(from.fileKey, from.contents, from.version),
          action?.conflict.startLineIndex,
        );
        // Stopped while the editor was on its way: it's put away again, nothing edited yet.
        if (disposed || !editing) {
          whole.stopEditing(true);
          return;
        }
        props.onEditing({
          version: from.version,
          discard: () => void stopEditing(true),
          hasSelection: edit.hasSelection,
        });
      } catch (error) {
        if (disposed || !editing) return;
        editing = false;
        props.onEditFailed(error instanceof Error ? error.message : String(error));
        void run();
      }
    };

    /**
     * Stops editing: shows the edits' conflicts, or the file as it was before them if they're
     * dropped, until it's read from disk again. Either way, its version on disk isn't known until
     * then: the edits were saved meanwhile, or the file changed on disk, which is why they're
     * dropped.
     */
    const stopEditing = async (discard: boolean) => {
      if (!editing) return;
      editing = false;
      const text = whole.stopEditing(discard);
      // Once whoever stopped it is done: dropping the edits, say, which it'd otherwise save.
      if (text !== undefined) queueMicrotask(() => props.onEditing(undefined));
      if (shown) {
        await showText(shown.fileKey, !discard && text !== undefined ? text : shown.contents, null);
      }
      void run();
    };

    createEffect(
      on(
        () => [props.fileKey, props.disk] as const,
        ([fileKey, disk]) => {
          wanted = disk && { fileKey, disk };
          if (disk) void run();
        },
      ),
    );
    createEffect(
      on(
        () => props.editing,
        (edit) => void (edit ? startEditing() : stopEditing(false)),
        { defer: true },
      ),
    );
    createEffect(
      on(
        theme,
        (type) => {
          conflicts.setThemeType(type);
          whole.setThemeType(type);
        },
        { defer: true },
      ),
    );

    onCleanup(() => {
      disposed = true;
      props.onCommands(undefined);
      // Edits not saved yet are saved all the same (see `useFileEditing`).
      if (editing && whole.stopEditing(false) !== undefined) props.onEditing(undefined);
      whole.hide();
      conflicts.cleanUp();
      highlights.drop();
    });
  }

  return (
    <div
      ref={(el) => (scroller = el)}
      // Focusable, to scroll it with the keyboard, and step through the conflicts with it.
      tabIndex={0}
      aria-label="Conflicts"
      aria-keyshortcuts="[ ] O T B"
      class="group h-full min-h-0 overflow-auto bg-bg outline-none [--diffs-font-size:12px] [--diffs-line-height:20px] focus-visible:shadow-[inset_0_0_0_1px_var(--primary)]"
    >
      <div ref={(el) => (conflictsWrapper = el)} class="[&>*]:min-h-px" />
      <div ref={(el) => (fileWrapper = el)} hidden class="[&>*]:min-h-px" />
    </div>
  );
}

/** The app's colours (see `APP_CSS`), and the sides' markers named as the buttons name them. */
const CONFLICT_CSS = `${APP_CSS}
[data-merge-conflict=marker-start]:after {
  content: "(ours)";
}
[data-merge-conflict=marker-end]:after {
  content: "(theirs)";
}`;

/**
 * Parses and highlights a conflicted file's conflicts ahead of showing them, as the viewer names
 * them, so they show at once. Nothing for one without conflicts, or whose markers can't be read.
 */
export function prepareConflicts(
  fileKey: string,
  path: string,
  contents: string,
  version: string,
): void {
  let state: ConflictState | undefined;
  try {
    state = parseConflicts(path, contents, conflictsCacheKey(fileKey, contents, version));
  } catch {
    // Said once it's opened.
  }
  if (state)
    void workerPool()
      .primeDiffHighlightCache(state.diff)
      .catch(() => undefined);
}
