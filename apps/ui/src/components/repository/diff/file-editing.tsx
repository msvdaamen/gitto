import FileWarning from "lucide-solid/icons/file-exclamation-point";
import Pencil from "lucide-solid/icons/pencil";
import { createSignal, onCleanup, onMount, Show, type JSX } from "solid-js";

import { Button, IconButton } from "@/components/ui/button";
import { saveWorkingTreeFile } from "@/git/queries/file-diff";

import { createAutosave } from "./autosave";
import type { EditSession } from "./patch-viewer";
import { UnsavedChangesDialog, type UnsavedChoice } from "./unsaved-changes-dialog";

/**
 * Editing an unstaged file in its diff: its new side, which is the file on disk, saved a moment
 * after each edit. Edit mode starts with the header's button and ends with it or Esc; leaving it,
 * the file or the repository saves what's left first, and asks what to do if that fails, e.g. as
 * the file changed on disk meanwhile.
 */
export function useFileEditing(props: {
  repositoryId: () => string;
  /** The file to edit, by its path in the repository. */
  path: () => string;
}) {
  const [editing, setEditing] = createSignal(false);
  const [session, setSession] = createSignal<EditSession>();
  /** Why editing couldn't start. */
  const [startError, setStartError] = createSignal<string>();
  /**
   * The file to edit again once it's on show as it is on disk, after its edits were dropped: only
   * with that file's next patch, and not once another file is opened, or this one has no changes
   * to show (see `cancelReload`), so no file is edited without being asked.
   */
  let reloading: string | undefined;
  // Saved to the file being edited when editing started, whichever is on show by then.
  const [editedPath, setEditedPath] = createSignal("");
  const autosave = createAutosave((text, version, overwrite) =>
    saveWorkingTreeFile(props.repositoryId(), editedPath(), text, version, overwrite),
  );

  // Asking what to do with edits that couldn't be saved; resolved with the user's choice.
  const [asking, setAsking] = createSignal<(choice: UnsavedChoice) => void>();
  const ask = () => new Promise<UnsavedChoice>((resolve) => setAsking(() => resolve));
  const choose = (choice: UnsavedChoice) => {
    const resolve = asking();
    setAsking(undefined);
    resolve?.(choice);
  };

  function start() {
    setStartError(undefined);
    setEditedPath(props.path());
    setEditing(true);
  }

  /** Stops editing, dropping the edits that aren't saved. */
  function discard() {
    session()?.discard();
    autosave.stop();
    setEditing(false);
  }

  /**
   * Stops editing, once what's left is saved, or the user says what to do with it; resolves to
   * whether editing stopped (or wasn't on), which leaving the file waits for. Asked again before
   * that's known (Esc, then the back button, say), it's the same answer.
   */
  let leaving: Promise<boolean> | undefined;
  function leave(): Promise<boolean> {
    leaving ??= saveAndStop().finally(() => (leaving = undefined));
    return leaving;
  }

  async function saveAndStop(): Promise<boolean> {
    if (!editing()) return true;
    if (!(await autosave.flush())) {
      const choice = await ask();
      if (choice === "cancel") return false;
      if (choice === "discard") {
        discard();
        return true;
      }
      const saved =
        autosave.state().kind === "changed-on-disk"
          ? await autosave.overwrite()
          : await autosave.flush();
      if (!saved) return false;
    }
    autosave.stop();
    setEditing(false);
    return true;
  }

  /** Drops the edits, and edits the file as it is on disk once that's on show. */
  function reload() {
    reloading = editedPath();
    discard();
  }

  // Esc leaves edit mode, before the editor takes it to collapse the selection: only when there's
  // one to collapse, or the editor's search is open (which Esc closes), is it left to the editor.
  const onEscape = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || !editing() || asking()) return;
    const target = event.composedPath()[0];
    if (target instanceof HTMLElement) {
      if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;
      const searching =
        target.getRootNode() instanceof ShadowRoot &&
        (target.getRootNode() as ShadowRoot).querySelector("[data-search-panel]") !== null;
      if (target.isContentEditable && (searching || session()?.hasSelection())) return;
    }
    event.preventDefault();
    event.stopPropagation();
    void leave();
  };
  // Edits not saved yet as the window closes are saved as it does: sent before the page goes.
  const onUnload = () => autosave.flushNow();
  onMount(() => {
    window.addEventListener("keydown", onEscape, true);
    window.addEventListener("beforeunload", onUnload);
    onCleanup(() => {
      window.removeEventListener("keydown", onEscape, true);
      window.removeEventListener("beforeunload", onUnload);
    });
  });
  // Edits made just before the file closed by itself (its row went, say) are saved all the same.
  onCleanup(() => void autosave.flush());

  /** How saving is going, next to the edit mode's button, once there's been something to save. */
  const [edited, setEdited] = createSignal(false);
  const status = () => {
    const kind = autosave.state().kind;
    if (kind === "pending" || kind === "saving") return "Saving…";
    return kind === "saved" && edited() ? "Saved" : undefined;
  };

  return {
    editing,
    start,
    leave,
    /** What the viewer takes to edit the file. */
    viewer: {
      onEditing: (next: EditSession | undefined) => {
        setSession(next);
        setEdited(false);
        if (next) autosave.start(next.version);
        else if (editing()) {
          // Ended without being asked to (the viewer went): what's left is saved all the same.
          setEditing(false);
          void autosave.flush().then(() => autosave.stop());
        }
      },
      onEdit: (text: string) => {
        setEdited(true);
        autosave.change(text);
      },
      onEditFailed: (message: string) => {
        setEditing(false);
        setStartError(message);
      },
    },
    /** The patch went off the page, e.g. as the file has no changes left: no reload to finish. */
    cancelReload() {
      reloading = undefined;
    },
    /** A patch was shown: the file as it's on disk, after a reload. */
    onShown() {
      const reloaded = reloading;
      reloading = undefined;
      if (reloaded !== undefined && reloaded === props.path()) start();
    },
    /** The edit mode's button, and how saving is going. */
    Controls(controlProps: { editable: boolean }) {
      return (
        <>
          <Show when={editing() && status()}>
            {(text) => (
              <span role="status" class="mr-1 text-[11px] text-faint">
                {text()}
              </span>
            )}
          </Show>
          <IconButton
            label={editing() ? "Stop editing (Esc)" : "Edit file"}
            icon={Pencil}
            active={editing()}
            disabled={!editing() && !controlProps.editable}
            onClick={() => (editing() ? void leave() : start())}
          />
        </>
      );
    },
    /** Why the edits aren't saved, or editing didn't start, with what can be done about it. */
    Banner() {
      const state = autosave.state;
      return (
        <>
          <Show when={editing() && state()} keyed>
            {(current) => (
              <Show when={current.kind === "changed-on-disk" || current.kind === "failed"}>
                <EditNotice message={"message" in current ? current.message : ""}>
                  <Show
                    when={current.kind === "changed-on-disk"}
                    fallback={
                      <Button variant="ghost" onClick={() => void autosave.flush()}>
                        Try again
                      </Button>
                    }
                  >
                    <Button variant="ghost" onClick={reload}>
                      Reload
                    </Button>
                    <Button variant="ghost" onClick={() => void autosave.overwrite()}>
                      Overwrite
                    </Button>
                  </Show>
                </EditNotice>
              </Show>
            )}
          </Show>
          <Show when={startError()}>
            {(message) => <EditNotice message={`Couldn't edit the file: ${message()}`} />}
          </Show>
          <UnsavedChangesDialog
            open={asking() !== undefined}
            path={editedPath()}
            reason={"message" in state() ? (state() as { message: string }).message : ""}
            changedOnDisk={state().kind === "changed-on-disk"}
            onChoose={choose}
          />
        </>
      );
    },
  };
}

/** A line under the header about the edits. */
function EditNotice(props: { message: string; children?: JSX.Element }) {
  return (
    <p
      role="status"
      class="m-0 flex shrink-0 items-center gap-1.5 border-b border-border px-3 py-1 text-[11.5px] text-muted"
    >
      <FileWarning size={13} class="shrink-0 text-amber" />
      <span class="mr-auto">{props.message}</span>
      {props.children}
    </p>
  );
}
