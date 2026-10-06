// A conflicted file shown whole, in place of its conflicts (see `ConflictViewer`): one without
// conflicts left, or whose markers can't be read as conflicts, and one being edited by hand, markers
// and all, with the library's editor.
import { VirtualizedFile, Virtualizer, type FileContents, type ThemeTypes } from "@pierre/diffs";
import type * as EditModule from "@pierre/diffs/edit";
import type { Editor } from "@pierre/diffs/edit";

import { APP_CSS, HIGHLIGHT_WAIT_MS, THEMES, workerPool } from "./viewer-runtime";

let editorModule: Promise<typeof EditModule> | undefined;
/** The library's editor, loaded the first time a file's edited. */
const loadEditor = () => (editorModule ??= import("@pierre/diffs/edit"));

/** What the whole file needs of the viewer it's shown in. */
export interface WholeFileDeps {
  /** The view's scroll container. */
  root: HTMLElement;
  /** The element the conflicts are shown in, hidden while the whole file is. */
  conflicts: HTMLElement;
  /** The element the whole file is shown in. */
  wrapper: HTMLElement;
  path: () => string;
  theme: () => ThemeTypes;
  /** Whether the viewer was taken off the page. */
  disposed: () => boolean;
  /** The file was edited to `text`. */
  onEdit: (text: string) => void;
}

/** A file being edited by hand (see `WholeFile.edit`). */
export interface WholeFileEdit {
  /** Whether there's a selection for Esc to collapse, or several to make one, in the editor. */
  hasSelection: () => boolean;
}

/** The whole file, in place of the conflicts (see `createWholeFile`). */
export interface WholeFile {
  /** Shows `contents` whole, highlighted first, named `cacheKey` in the highlighting cache. */
  show: (contents: string, cacheKey: string) => Promise<void>;
  /** Shows the conflicts again, in its place. */
  hide: () => void;
  /**
   * Shows `contents` whole and edits it, at line `line` (from 0) if given, a third of the way
   * down the view, where the conflict it's of was among the conflicts.
   */
  edit: (contents: string, cacheKey: string, line: number | undefined) => Promise<WholeFileEdit>;
  /**
   * Stops editing, if it was, keeping the edits unless `discard`; resolves to the text they left,
   * `undefined` if it wasn't being edited.
   */
  stopEditing: (discard: boolean) => string | undefined;
  setThemeType: (type: ThemeTypes) => void;
}

/** The whole file, shown through a virtualizer of its own, as only the lines on screen are. */
export function createWholeFile(deps: WholeFileDeps): WholeFile {
  let view: { file: VirtualizedFile<undefined>; virtualizer: Virtualizer } | undefined;
  let editor: { opened: Editor<"file", undefined>; finish: () => void } | undefined;

  const options = () => ({
    theme: THEMES,
    themeType: deps.theme(),
    disableFileHeader: true,
    unsafeCSS: APP_CSS,
    onEditChange: (event: { file: FileContents }) => deps.onEdit(event.file.contents),
    // Shown afresh once editing stops: the edits' conflicts are (see `ConflictViewer`).
    onEditComplete: () => "reject" as const,
  });

  const show = async (contents: string, cacheKey: string) => {
    const file = { name: deps.path(), contents, cacheKey };
    await Promise.race([
      workerPool()
        .primeFileHighlightCache(file)
        .catch(() => undefined),
      new Promise((resolve) => setTimeout(resolve, HIGHLIGHT_WAIT_MS)),
    ]);
    if (deps.disposed()) return;
    if (!view) {
      const virtualizer = new Virtualizer();
      virtualizer.setup(deps.root, deps.wrapper);
      view = {
        file: new VirtualizedFile(options(), virtualizer, undefined, workerPool()),
        virtualizer,
      };
    }
    deps.conflicts.hidden = true;
    deps.wrapper.hidden = false;
    view.file.render({ file, containerWrapper: deps.wrapper });
  };

  const hide = () => {
    deps.conflicts.hidden = false;
    deps.wrapper.hidden = true;
    if (!view) return;
    view.file.cleanUp();
    view.virtualizer.cleanUp();
    view = undefined;
  };

  const edit = async (contents: string, cacheKey: string, line: number | undefined) => {
    const [{ Editor: EditorClass }] = await Promise.all([loadEditor(), show(contents, cacheKey)]);
    if (deps.disposed() || !view) throw new Error("The file is no longer on show.");
    // Focused once the editor's on the page: before that, there's nowhere to scroll it to.
    const opened = new EditorClass<"file", undefined>("file", {
      onAttach: (attached) =>
        attached.focus(
          line === undefined
            ? { lineNumber: "first-visible" }
            : { lineNumber: line + 1, offset: deps.root.clientHeight / 3 },
        ),
    });
    editor = { opened, finish: opened.edit(view.file) };
    return {
      hasSelection: () => {
        const selections = opened.getViewState().selections ?? [];
        const [first] = selections;
        if (selections.length !== 1 || !first) return selections.length > 1;
        return first.start.line !== first.end.line || first.start.character !== first.end.character;
      },
    };
  };

  const stopEditing = (discard: boolean) => {
    if (!editor) return undefined;
    const { opened, finish } = editor;
    editor = undefined;
    const text = opened.getText();
    if (discard) opened.cleanUp("discard");
    else finish();
    return text;
  };

  return {
    show,
    hide: () => {
      stopEditing(false);
      hide();
    },
    edit,
    stopEditing,
    setThemeType: (type) => view?.file.setThemeType(type),
  };
}
