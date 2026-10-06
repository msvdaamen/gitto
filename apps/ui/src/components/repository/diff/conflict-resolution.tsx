import type { ChangedFile, Conflict } from "@gitto/git/types";
import { useQueryClient, type QueryClient } from "@tanstack/solid-query";
import Check from "lucide-solid/icons/check";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronUp from "lucide-solid/icons/chevron-up";
import FileCheck from "lucide-solid/icons/file-check";
import GitMergeConflict from "lucide-solid/icons/git-merge-conflict";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import SkipForward from "lucide-solid/icons/skip-forward";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import {
  createEffect,
  createMemo,
  createSignal,
  ErrorBoundary,
  lazy,
  Match,
  on,
  Show,
  Suspense,
  Switch,
} from "solid-js";

import { Button, IconButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Notice } from "@/components/ui/notice";
import { conflictKind, describeConflict } from "@/git/conflicts";
import {
  conflictQuery,
  useConflict,
  useResolveFile,
  type ConflictResolution,
} from "@/git/queries/conflicts";
import { saveWorkingTreeFile } from "@/git/queries/file-diff";
import { gitKeys } from "@/git/queries/keys";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useDelayed } from "@/hooks/delayed";
import { isTyping } from "@/lib/typing";

import { ConflictSides } from "./conflict-sides";
import type * as ViewerModule from "./conflict-viewer";
import type { ConflictCommands, ConflictProgress, Resolution } from "./conflict-viewer";
import { MarkersDialog } from "./markers-dialog";
import type { EditSession } from "./viewer-editing";

let viewerModule: Promise<typeof ViewerModule> | undefined;
/** The conflicts' viewer's module, loaded once, the first time it's needed: it brings Shiki. */
const loadViewer = () => (viewerModule ??= import("./conflict-viewer"));
const ConflictViewer = lazy(loadViewer);

/**
 * Loads a conflicted file ahead of opening it, e.g. when it's pointed at, and highlights its
 * conflicts, so they show at once; the first time, that starts the viewer too.
 */
export function prefetchConflict(
  client: QueryClient,
  repositoryId: string,
  fileKey: string,
  path: string,
): void {
  void Promise.all([client.fetchQuery(conflictQuery(repositoryId, path)), loadViewer()])
    .then(([conflict, viewer]) => {
      const kind = conflictKind(conflict);
      return kind.kind === "text"
        ? viewer.prepareConflicts(fileKey, path, kind.contents, kind.version)
        : undefined;
    })
    // Only ahead of time: opening it says what went wrong.
    .catch(() => undefined);
}

/**
 * Resolving a conflicted file in its view: conflict by conflict when both sides changed it as text
 * (see `ConflictViewer`), or by keeping one side whole, and then marking it resolved, which stages
 * it. Only once it has no conflict markers left; never by itself. Once it's resolved, the next
 * conflicted file opens, or this one's staged changes if it was the last.
 *
 * In the view, `[` and `]` step through the conflicts, `O`, `T` and `B` keep ours, theirs or both
 * of the current one, `N` opens the next conflicted file, and ⌘↵ marks the file resolved.
 */
export function useConflictResolution(props: {
  repositoryId: () => string;
  /** The file on show, as its list has it now. */
  file: () => ChangedFile;
  /** Names it among the files on show (see `diffFileKey`). */
  fileKey: () => string;
  /** The files in its list, to step to the next conflicted one. */
  files: () => ChangedFile[];
  /** Whether it's being edited by hand (see `useFileEditing`). */
  editing: () => boolean;
  /** What the viewer tells of editing (see `useFileEditing`'s `viewer`). */
  viewer: {
    onEditing: (session: EditSession | undefined) => void;
    onEdit: (text: string) => void;
    onEditFailed: (message: string) => void;
  };
  /** The file read last is on show, e.g. to edit it again after a reload. */
  onShown: () => void;
  /** Opens another uncommitted file: the next conflicted one, or this one's staged changes. */
  open: (source: { kind: "unstaged" | "staged" }, file: ChangedFile) => void;
}) {
  const queryClient = useQueryClient();
  const active = () => props.file().status === "conflicted";
  const path = () => props.file().path;
  const query = useConflict(props.repositoryId, path, active);
  // Without Suspense: it's refetched while it's on show (see `useUnsuspendedData`).
  const data = useUnsuspendedData(query);
  const kind = createMemo(() => {
    const conflict = data();
    return conflict && conflictKind(conflict);
  });

  // After editing, the file is read again before it's shown: what was read before is of the edits
  // saved so far, not the last.
  const [readSince, setReadSince] = createSignal(0);
  createEffect(
    on(
      props.editing,
      (now, was) => {
        if (!was || now) return;
        setReadSince(Date.now());
        void query.refetch();
      },
      { defer: true },
    ),
  );
  // Not the last file's, still on show while this one's loads: the viewer would take it for this
  // one's.
  const disk = createMemo(() => {
    const current = kind();
    if (props.editing() || current?.kind !== "text" || query.isPlaceholderData) return undefined;
    if (query.dataUpdatedAt < readSince()) return undefined;
    return { contents: current.contents, version: current.version };
  });

  const [progress, setProgress] = createSignal<ConflictProgress>();
  /**
   * The viewer's progress, once it's of this file: the last one's is still on show while this
   * one's conflicts are being highlighted.
   */
  const shownProgress = () => {
    const shown = progress();
    return kind()?.kind === "text" && shown?.fileKey === props.fileKey() ? shown : undefined;
  };
  const [commands, setCommands] = createSignal<ConflictCommands>();
  /** Why the last thing done to the conflict failed, and of which file. */
  const [error, setError] = createSignal<{ key: string; message: string }>();
  const failed = (key: string, reason: unknown) =>
    setError({ key, message: reason instanceof Error ? reason.message : String(reason) });

  const resolveFile = useResolveFile();
  const [resolving, setResolving] = createSignal(false);
  // Nor while the last file's still on show, as this one's loads, nor before this one's version
  // on disk is known, which a write is checked against: just after editing, say. Nor while it's
  // edited: it's resolved as the edits leave it, which is only known once editing stops.
  const busy = () =>
    resolving() ||
    props.editing() ||
    query.isPlaceholderData ||
    (kind()?.kind === "text" &&
      (!shownProgress() || shownProgress()!.saving || !shownProgress()!.version));

  /** The next conflicted file in the list after this one, or the first before it. */
  const nextConflicted = () => {
    const files = props.files();
    const at = files.findIndex((file) => file.path === path());
    const ordered = at === -1 ? files : [...files.slice(at + 1), ...files.slice(0, at)];
    return ordered.find((file) => file.status === "conflicted" && file.path !== path());
  };

  /**
   * Whether it has no conflicts left in its text, so it's ready to be marked resolved. One that
   * has is only marked resolved with them once the user says they belong in it (`confirming`).
   */
  const ready = () => {
    const shown = shownProgress();
    return !!shown && shown.left === 0 && !shown.problem;
  };
  /** Asking whether to mark it resolved with its markers left, as they belong in it. */
  const [confirming, setConfirming] = createSignal(false);

  /**
   * Resolves the file whole: marks it resolved, or keeps a side; then on to the next conflicted
   * file, or to its staged changes if it was the last. Not while it's edited (see `busy`).
   */
  const resolveWhole = async (resolution: ConflictResolution) => {
    if (busy()) return;
    const key = props.fileKey();
    const file = props.file();
    setResolving(true);
    try {
      // Picked while the file's still in its list: it goes once it's resolved.
      const target = nextConflicted();
      setError(undefined);
      try {
        await resolveFile.mutateAsync({
          repositoryId: props.repositoryId(),
          path: file.path,
          resolution,
        });
      } catch (reason) {
        failed(key, reason);
        return;
      }
      if (key !== props.fileKey()) return;
      if (target) props.open({ kind: "unstaged" }, target);
      else props.open({ kind: "staged" }, { ...file, status: "modified" });
    } finally {
      setResolving(false);
    }
  };

  /** The version of the file to check before resolving it whole: the latest one written here. */
  const version = () => shownProgress()?.version ?? data()?.version ?? null;
  const markResolved = () => {
    if (kind()?.kind !== "text" || busy()) return;
    if (ready()) void resolveWhole({ action: "mark", version: version(), withMarkers: false });
    else setConfirming(true);
  };
  const keep = ({ base, ours, theirs }: Conflict, side: "ours" | "theirs") =>
    void resolveWhole({
      action: "keep",
      side,
      sides: { base, ours, theirs },
      version: version(),
    });

  const save = async (contents: string, at: string) => {
    try {
      return await saveWorkingTreeFile(props.repositoryId(), path(), contents, at, false);
    } catch (reason) {
      // Changed on disk since, say: shown as it is now.
      void queryClient.invalidateQueries({
        queryKey: gitKeys.conflict(props.repositoryId(), path()),
      });
      throw reason;
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (!active() || event.defaultPrevented || props.editing() || isTyping(event)) return;
    // Not for what's behind a dialog or a menu that has the keys.
    if (inOverlay(event)) return;
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      markResolved();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const resolutions: Record<string, Resolution> = { o: "current", t: "incoming", b: "both" };
    const key = event.key.toLowerCase();
    if (key === "n") {
      const target = nextConflicted();
      if (!target) return;
      event.preventDefault();
      props.open({ kind: "unstaged" }, target);
    } else if (event.key === "]" || event.key === "[") {
      event.preventDefault();
      if (event.key === "]") commands()?.next();
      else commands()?.previous();
    } else if (resolutions[key] && !busy() && kind()?.kind === "text") {
      event.preventDefault();
      commands()?.resolve(resolutions[key]);
    }
  };

  return {
    active,
    /** Whether it can be edited by hand: its text, once it's on show as it is on disk. */
    editable: () =>
      kind()?.kind === "text" && !!shownProgress()?.version && !query.isPlaceholderData,
    onKeyDown,

    /** The conflicts left, stepping through them, marking the file resolved, and the next file. */
    Controls() {
      const left = () => shownProgress()?.left;
      return (
        <>
          <Show when={left() !== undefined && !shownProgress()?.problem}>
            <span
              role="status"
              class="mr-1 text-[11px] whitespace-nowrap text-faint"
              aria-label={left() === 0 ? "No conflicts left" : `${left()} conflicts left`}
            >
              {left() === 0
                ? "No conflicts left"
                : `${left()} ${left() === 1 ? "conflict" : "conflicts"} left`}
            </span>
            <IconButton
              label="Previous conflict ([)"
              icon={ChevronUp}
              disabled={!left() || props.editing()}
              onClick={() => commands()?.previous()}
            />
            <IconButton
              label="Next conflict (])"
              icon={ChevronDown}
              disabled={!left() || props.editing()}
              onClick={() => commands()?.next()}
            />
          </Show>
          <Show when={kind()?.kind === "text"}>
            <Button
              icon={Check}
              variant={ready() ? "primary" : "secondary"}
              disabled={busy()}
              class="ml-1 h-[26px] gap-1.5 rounded-md px-2 text-[11.5px] font-[600] shadow-none enabled:hover:translate-y-0"
              onClick={markResolved}
            >
              Mark resolved
            </Button>
          </Show>
          <IconButton
            label="Next conflicted file (N)"
            icon={SkipForward}
            disabled={!nextConflicted()}
            onClick={() => {
              const target = nextConflicted();
              if (target) props.open({ kind: "unstaged" }, target);
            }}
          />
        </>
      );
    },

    /** What each side did with the file, and keeping one whole; or why that failed. */
    Banner() {
      return (
        <>
          <Show when={kind()?.kind === "text" && data()} keyed>
            {(shown) => (
              <Notice
                icon={GitMergeConflict}
                message={shownProgress()?.problem ?? describeConflict(shown)}
              >
                <Button variant="ghost" disabled={busy()} onClick={() => keep(shown, "ours")}>
                  Keep all of ours
                </Button>
                <Button variant="ghost" disabled={busy()} onClick={() => keep(shown, "theirs")}>
                  Keep all of theirs
                </Button>
              </Notice>
            )}
          </Show>
          <Show when={error()?.key === props.fileKey() && error()}>
            {(shown) => <Notice role="alert" icon={TriangleAlert} message={shown().message} />}
          </Show>
          <MarkersDialog
            open={confirming()}
            path={path()}
            left={shownProgress()?.left ?? 0}
            onCancel={() => setConfirming(false)}
            onConfirm={() => {
              setConfirming(false);
              void resolveWhole({ action: "mark", version: version(), withMarkers: true });
            }}
          />
        </>
      );
    },

    /** The conflicts, or the sides to keep one of. */
    View() {
      return (
        <Switch fallback={<Loading />}>
          <Match when={query.error} keyed>
            {(reason) => (
              <EmptyState
                icon={TriangleAlert}
                title="Couldn't load the conflict"
                tone="error"
                class="h-full"
              >
                {reason.message}
              </EmptyState>
            )}
          </Match>
          <Match when={kind()?.kind === "resolved"}>
            <EmptyState icon={FileCheck} title="No longer conflicted" class="h-full">
              This file's conflict was resolved.
            </EmptyState>
          </Match>
          <Match when={kind()?.kind === "sides" && data()} keyed>
            {(shown) => (
              <ConflictSides
                conflict={shown}
                disabled={busy()}
                onKeep={(side) => keep(shown, side)}
              />
            )}
          </Match>
          <Match when={kind()?.kind === "text"}>
            <ErrorBoundary
              fallback={(reason: Error) => (
                <EmptyState
                  icon={TriangleAlert}
                  title="Couldn't show the conflicts"
                  tone="error"
                  class="h-full"
                >
                  {reason.message}
                </EmptyState>
              )}
            >
              <Suspense fallback={<Loading />}>
                <ConflictViewer
                  fileKey={props.fileKey()}
                  path={path()}
                  disk={disk()}
                  save={save}
                  editing={props.editing()}
                  onEditing={props.viewer.onEditing}
                  onEdit={props.viewer.onEdit}
                  onEditFailed={props.viewer.onEditFailed}
                  onProgress={setProgress}
                  onCommands={setCommands}
                  onError={(message) =>
                    message === undefined ? setError(undefined) : failed(props.fileKey(), message)
                  }
                  onShown={props.onShown}
                />
              </Suspense>
            </ErrorBoundary>
          </Match>
        </Switch>
      );
    },
  };
}

/** Shown while the conflict loads, unless it's quick about it. */
function Loading() {
  const shown = useDelayed(() => true, 150);
  return (
    <Show when={shown()}>
      <EmptyState icon={LoaderCircle} loading title="Loading the conflict…" class="h-full" />
    </Show>
  );
}

/**
 * Whether a key pressed in `event`'s target is for a dialog, a menu or a list that's open, rather
 * than for the view behind it.
 */
function inOverlay(event: KeyboardEvent) {
  const target = event.composedPath()[0];
  const overlay = '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]';
  if (target instanceof Element && target.closest(overlay)) return true;
  return document.querySelector('[aria-modal="true"], [role="menu"]') !== null;
}
