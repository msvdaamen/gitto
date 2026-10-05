import { createSignal } from "solid-js";

/** Where saving a file that's being edited is at. */
export type SaveState =
  | { kind: "saved" }
  /** Edited since the last save, which is due once the typing stops. */
  | { kind: "pending" }
  | { kind: "saving" }
  /** The file changed on disk since it was read: not saved until the user says what to keep. */
  | { kind: "changed-on-disk"; message: string }
  /** Saving failed otherwise, e.g. the disk is full; tried again with the next edit. */
  | { kind: "failed"; message: string };

/** Saves `text` over the file as it was at `version`, or whatever it is if `overwrite`. */
export type SaveFile = (text: string, version: string, overwrite: boolean) => Promise<string>;

/** How long after the last edit it's saved: long enough not to write on every key. */
export const AUTOSAVE_DELAY_MS = 600;

/**
 * Saves a file's edits a moment after each, one save at a time, each over the version the last
 * one wrote: one that changed on disk since isn't saved over unless the user says so (`overwrite`).
 * Started with the version the edits are of; `flush` saves what's left at once, e.g. before the
 * file is closed.
 */
export function createAutosave(save: SaveFile, delayMs = AUTOSAVE_DELAY_MS) {
  const [state, setState] = createSignal<SaveState>({ kind: "saved" });
  let version: string | undefined;
  /** The edits to save; `undefined` once they're saved. */
  let unsaved: string | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** The saves asked for, one after the other. */
  let saves: Promise<void> = Promise.resolve();

  /** Saves the latest edits, after any save under way. */
  function run(overwrite: boolean): Promise<void> {
    clearTimeout(timer);
    timer = undefined;
    saves = saves.then(() => saveLatest(overwrite));
    return saves;
  }

  async function saveLatest(overwrite: boolean): Promise<void> {
    const text = unsaved;
    if (text === undefined || version === undefined) return;
    if (!overwrite && state().kind === "changed-on-disk") return;
    setState({ kind: "saving" });
    try {
      version = await save(text, version, overwrite);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setState(
        isChangedOnDisk(error) ? { kind: "changed-on-disk", message } : { kind: "failed", message },
      );
      return;
    }
    if (unsaved === text) {
      unsaved = undefined;
      setState({ kind: "saved" });
    } else {
      // Edited again meanwhile.
      setState({ kind: "pending" });
      schedule();
    }
  }

  function schedule() {
    clearTimeout(timer);
    timer = setTimeout(() => void run(false), delayMs);
  }

  return {
    state,
    /** Saves over the file as it is at `version` from now on, with nothing to save yet. */
    start(at: string) {
      clearTimeout(timer);
      version = at;
      unsaved = undefined;
      setState({ kind: "saved" });
    },
    /** The file was edited to `text`: saved once the edits stop for a moment. */
    change(text: string) {
      unsaved = text;
      if (state().kind === "changed-on-disk") return;
      setState({ kind: "pending" });
      schedule();
    },
    /** Saves the edits that are left now; resolves to whether everything's saved. */
    async flush(): Promise<boolean> {
      await (unsaved === undefined ? saves : run(false));
      return unsaved === undefined && state().kind === "saved";
    },
    /**
     * Sends the edits that are left at once, as the window closes: not after a save under way,
     * whose answer the window won't be there to get. Over the version that save started from,
     * which the main process takes as the one it's saving (see `saveWorkingTreeFile`).
     */
    flushNow() {
      clearTimeout(timer);
      if (unsaved === undefined || version === undefined) return;
      if (state().kind === "changed-on-disk") return;
      void save(unsaved, version, false).catch(() => undefined);
    },
    /** Saves the edits over the file whatever it is now; resolves to whether they're saved. */
    async overwrite(): Promise<boolean> {
      await run(true);
      return unsaved === undefined && state().kind === "saved";
    },
    /** Forgets the edits that aren't saved, and stops saving. */
    stop() {
      clearTimeout(timer);
      timer = undefined;
      unsaved = undefined;
      version = undefined;
      setState({ kind: "saved" });
    },
    /** Whether there are edits that aren't saved. */
    dirty: () => unsaved !== undefined,
  };
}

export type Autosave = ReturnType<typeof createAutosave>;

/**
 * Whether saving failed because the file changed, or went, on disk since it was read: the main
 * process's error for that (see `toApiError`), which an overwrite doesn't get.
 */
function isChangedOnDisk(error: unknown): boolean {
  return (error as { code?: unknown } | null)?.code === "CONFLICT";
}
