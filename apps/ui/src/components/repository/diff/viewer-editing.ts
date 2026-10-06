// Editing the file on show in the viewer, in place, with the library's editor (see `PatchViewer`'s
// `editing`): starting from the whole file, and leaving the edits on show once it stops.
import { parseDiffFromFile, type FileDiffMetadata } from "@pierre/diffs";
import type * as EditModule from "@pierre/diffs/edit";
import type { Editor } from "@pierre/diffs/edit";

import type { HunkButton } from "./line-staging";
import { carriedExpansion, fitToLines, type Side } from "./patch-files";
import {
  EXPAND_HIGHLIGHT_WAIT_MS,
  nextEditedDiff,
  type LoadedFile,
  type ViewerFileDiff,
  type ViewHighlights,
} from "./viewer-runtime";

/** Editing the file on show: what it started from, and what can be done with it meanwhile. */
export interface EditSession {
  /** The version of the file on disk the edits are of (see `readWorkingTreeFile`). */
  version: string;
  /** Drops the edits made since the last save, and stops editing. */
  discard: () => void;
  /** Whether there's a selection for Esc to collapse, or several to make one, in the editor. */
  hasSelection: () => boolean;
}

let editorModule: Promise<typeof EditModule> | undefined;
/** The library's editor, loaded the first time a file's edited. */
const loadEditor = () => (editorModule ??= import("@pierre/diffs/edit"));

/** What editing needs of the viewer: its diff, what's on show in it, and what to tell of edits. */
export interface ViewerEditingDeps {
  diffs: ViewerFileDiff;
  /** The element the diff is rendered in. */
  wrapper: HTMLDivElement;
  /** What the view had highlighted, which the whole files and the edits are added to. */
  highlights: ViewHighlights;
  /** The diff on show, once there's one. */
  shownDiff: () => FileDiffMetadata | undefined;
  /** Puts `diff` on show in the last one's place: the whole file to edit, or the edits left. */
  setShownDiff: (diff: FileDiffMetadata) => void;
  /** Names the file on show (see `PatchViewer`'s `fileKey`). */
  shownFileKey: () => string | undefined;
  /** Whether the viewer was taken off the page. */
  disposed: () => boolean;
  /** Reads one side of the file in full, by the object name the patch has for it. */
  loadFile: (side: Side, oid: string) => Promise<LoadedFile>;
  /** Editing started (see `EditSession`), or stopped. */
  onEditing: (session: EditSession | undefined) => void;
  /** Why editing couldn't start, e.g. as the file changed again meanwhile. */
  onEditFailed: (message: string) => void;
  /** Editing started: the diff on show is no longer the patch's. */
  onStarted: () => void;
  /** Editing stopped, with the edits or the patch on show: the next patch, if any, can be. */
  onStopped: () => void;
}

/** Editing the file on show (see `createViewerEditing`). */
export interface ViewerEditing {
  /** Whether the file on show is being edited, from when editing starts until it stops. */
  active: () => boolean;
  /**
   * Starts editing the file on show, once it's whole: a patch's diff is filled in with both files,
   * or for an added file, built from it. The lines shown around the changes stay, as the hunks are
   * the same.
   */
  start: () => Promise<void>;
  /**
   * Stops editing, keeping the edits on show unless they're dropped, then shows the last patch.
   * The edits are shown in a diff of the viewer's own, highlighted in the workers first, in the
   * editor's place: the library's would be highlighted on the main thread, once as editing stops
   * and again as it starts over (1.6s each for 2,500 lines, the window frozen).
   */
  stop: (discard: boolean) => Promise<void>;
  /** Finishes editing, if it's under way, as the viewer is taken off the page. */
  dispose: () => void;
}

/** Edits the file on show in `deps.diffs` (see `PatchViewer`'s `editing`). */
export function createViewerEditing(deps: ViewerEditingDeps): ViewerEditing {
  const { diffs, wrapper, highlights } = deps;
  /** The editor while editing, and how to finish it. */
  let editor: Editor<"file-diff", HunkButton> | undefined;
  let finishEditing: (() => void) | undefined;
  let editing = false;
  /** The whole file's diff the edits started from. */
  let editedFrom: FileDiffMetadata | undefined;

  const startEditing = async () => {
    const from = deps.shownDiff();
    if (!from || editing) return;
    editing = true;
    deps.onStarted();
    try {
      const [whole, { Editor }] = await Promise.all([
        wholeFile(from).then(async (file) => {
          // Highlighted in the workers first, which the editor starts from: it highlights the
          // whole file again on the main thread otherwise (1.6s for 2,500 lines), e.g. right
          // after editing stopped, while the edits are highlighted.
          await highlights.highlight(file.diff, EXPAND_HIGHLIGHT_WAIT_MS);
          return file;
        }),
        loadEditor(),
      ]);
      if (deps.disposed() || !editing || deps.shownDiff() !== from) return;
      deps.setShownDiff(whole.diff);
      editedFrom = whole.diff;
      diffs.render({ fileDiff: whole.diff, containerWrapper: wrapper, forceRender: true });
      // Without an `editStateKey`, which would keep the undo history for when the file's edited
      // again: picking it up has the library highlight the whole file again on the main thread
      // (1.6s for 2,500 lines, the window frozen), rather than start from the workers'.
      const opened = new Editor<"file-diff", HunkButton>("file-diff", {});
      editor = opened;
      finishEditing = opened.edit(diffs);
      opened.focus();
      deps.onEditing({
        version: whole.version,
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
      if (deps.disposed() || !editing) return;
      editing = false;
      deps.onEditFailed(error instanceof Error ? error.message : String(error));
    }
  };

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
    const session = diffs.nameEditedDiff(`${deps.shownFileKey()}:session:${nextEditedDiff()}`);
    highlights.remember(session);
    if (discard) opened?.cleanUp("discard");
    else finishEditing?.();
    finishEditing = undefined;
    if (left && session) {
      diffs.expanded = carriedExpansion(session, diffs.expanded, left);
      deps.setShownDiff(left);
      diffs.render({ fileDiff: left, containerWrapper: wrapper });
    }
    deps.onEditing(undefined);
    deps.onStopped();
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
      diff.cacheKey = `${deps.shownFileKey()}:edited:${nextEditedDiff()}`;
      // oxlint-disable-next-line no-await-in-loop -- the text it's of may have changed since.
      await highlights.highlight(diff, EXPAND_HIGHLIGHT_WAIT_MS);
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
      return { diff: whole, version: read.version };
    }
    if (diff.isPartial) {
      const { hydrated, version } = await highlights.loadWhole(diff, deps.loadFile);
      if (!version) throw new Error("This file can't be edited.");
      return { diff: hydrated, version };
    }
    // Whole already, as more of it was shown, or it was edited: still the file on disk? Throws if
    // it isn't.
    const read = await readNewSide(diff);
    fitToLines(diff.additionLines, read.contents);
    return { diff, version: read.version };
  };

  /** The working tree's side of `diff`, with its version. */
  const readNewSide = async (diff: FileDiffMetadata) => {
    const read = await deps.loadFile("new", diff.newObjectId ?? "");
    if (typeof read === "string") throw new Error("This file can't be edited.");
    return read;
  };

  const dispose = () => {
    if (!finishEditing) return;
    highlights.remember(diffs.nameEditedDiff(`${deps.shownFileKey()}:edited:${nextEditedDiff()}`));
    finishEditing();
    // Taken off the page while editing: that's over.
    deps.onEditing(undefined);
  };

  return { active: () => editing, start: startEditing, stop: stopEditing, dispose };
}
