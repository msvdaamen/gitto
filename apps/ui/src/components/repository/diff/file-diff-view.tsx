import type { ChangedFile } from "@gitto/git/types";
import { useQueryClient, type QueryClient } from "@tanstack/solid-query";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronUp from "lucide-solid/icons/chevron-up";
import Columns2 from "lucide-solid/icons/columns-2";
import FileCheck from "lucide-solid/icons/file-check";
import FileWarning from "lucide-solid/icons/file-exclamation-point";
import FolderGit from "lucide-solid/icons/folder-git-2";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import Rows2 from "lucide-solid/icons/rows-2";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import {
  children,
  createEffect,
  createMemo,
  ErrorBoundary,
  lazy,
  Match,
  on,
  onCleanup,
  onMount,
  Show,
  Suspense,
  Switch,
  type JSX,
} from "solid-js";

import { Button, IconButton } from "@/components/ui/button";
import { EmptyState, LoadingState } from "@/components/ui/empty-state";
import { FileStatusBadge } from "@/components/ui/file-status-badge";
import { LineStats } from "@/components/ui/line-stats";
import { Notice } from "@/components/ui/notice";
import {
  countsOf,
  hasPatch,
  sourceLabel,
  totalLines,
  unchangedReason,
  type LineCounts,
} from "@/git/changes";
import { diffFileKey, type DiffSource } from "@/git/diff-source";
import { fetchBlob } from "@/git/queries/diff";
import { fetchFilePatch, fetchWorkingTreeFile } from "@/git/queries/file-diff";
import { useDiffStyle } from "@/hooks/diff-style";
import { isTyping } from "@/lib/typing";

import { prefetchConflict, useConflictResolution } from "./conflict-resolution";
import { useFileEditing } from "./file-editing";
import { useFileStaging } from "./file-staging";
import type * as ViewerModule from "./patch-viewer";
import { createShownFile, patchCacheKey } from "./shown-file";

let viewerModule: Promise<typeof ViewerModule> | undefined;
/** The viewer's module, loaded once, the first time it's needed: it brings Shiki. */
const loadViewer = () => (viewerModule ??= import("./patch-viewer"));
const PatchViewer = lazy(loadViewer);

/**
 * Changed lines above which a file's changes aren't loaded ahead (see `prefetchFileDiff`): parsing
 * them on the main thread starts to take a while, for a file that may not be opened after all.
 */
const PREFETCH_DIFF_LINES = 5_000;

/**
 * Loads and highlights a file's changes ahead of opening them, e.g. when it's pointed at, so they
 * show at once; the first time, that starts the viewer too. Not for one that's binary, or large,
 * nor for one without line counts (untracked, say), which can be of any size: an untracked log
 * that's being written to would be read whole again each time it's pointed at. An uncommitted
 * file's are named by what's in them in the highlighting cache, and refetched like the status once
 * the working tree changes, so what's loaded ahead is never shown stale.
 */
export function prefetchFileDiff(
  client: QueryClient,
  repositoryId: string,
  source: DiffSource,
  file: ChangedFile,
  uncounted = false,
): void {
  if (file.status === "conflicted" && source.kind === "unstaged") {
    prefetchConflict(client, repositoryId, diffFileKey(source, file.path), file.path);
    return;
  }
  const counts = countsOf(file);
  if (!hasPatch(file, uncounted) || !counts || totalLines(counts) > PREFETCH_DIFF_LINES) return;
  void Promise.all([fetchFilePatch(client, repositoryId, source, file), loadViewer()])
    .then(([data, viewer]) =>
      data.patch ? viewer.preparePatch(data.patch, patchCacheKey(source, data)) : undefined,
    )
    // Only ahead of time: opening it says what went wrong.
    .catch(() => undefined);
}

/**
 * The changes in one file of a commit, a stash or the uncommitted changes, in place of the
 * history. Esc, or the back button, goes back to the history; the other files in the same list
 * are a step away. Uncommitted changes are kept up to date as they change on disk, in place; once
 * the file has none left on its side, that's said instead.
 *
 * An uncommitted file can be staged, or unstaged, whole, or its lines a hunk or a selection at a
 * time (see `useFileStaging`), one at a time; an unstaged one can be edited (see
 * `useFileEditing`). Once that leaves it without changes on its side, the next file in its list
 * opens. A conflicted one shows its conflicts instead, to resolve (see `useConflictResolution`).
 */
export function FileDiffView(props: {
  repositoryId: string;
  source: DiffSource;
  file: ChangedFile;
  /** Shows another file's changes, e.g. the next one in the list. */
  onOpen: (source: DiffSource, file: ChangedFile) => void;
  onClose: () => void;
  /**
   * Gets what to wait for before the file is closed, or another opened: edits to it saved, or the
   * user saying what to do with them. Resolves to whether to go on.
   */
  onGuard?: (guard: (() => Promise<boolean>) | undefined) => void;
}) {
  const { diffStyle } = useDiffStyle();
  const queryClient = useQueryClient();
  const fileEditing = useFileEditing({
    repositoryId: () => props.repositoryId,
    path: () => props.file.path,
  });
  props.onGuard?.(fileEditing.leave);
  onCleanup(() => props.onGuard?.(undefined));
  // What's shown of the file: the one place staging, editing and resolving conflicts read it from.
  const view = createShownFile({
    repositoryId: () => props.repositoryId,
    source: () => props.source,
    file: () => props.file,
    editing: fileEditing.editing,
  });
  const { fileKey, file, shown, previous, next } = view;
  /** Whether the file on show can be edited: its patch (see `editablePatch`), or its conflicts. */
  const editable = () => view.editablePatch() || conflict.editable();

  const fileStaging = useFileStaging(view, {
    repositoryId: () => props.repositoryId,
    leave: fileEditing.leave,
    open: (target) => props.onOpen(props.source, target),
  });
  const conflict = useConflictResolution(view, {
    repositoryId: () => props.repositoryId,
    viewer: fileEditing.viewer,
    onShown: () => fileEditing.onShown(),
    open: (side, target) => props.onOpen(side, target),
  });

  let section: HTMLElement | undefined;
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented || isTyping(event)) return;
    event.preventDefault();
    props.onClose();
  };
  onMount(() => {
    // On the window, so it hears Esc after a popover, menu or dialog has (on the document): one
    // that closes on it marks it handled, and the changes stay open. So are the conflicts' keys,
    // wherever the focus is but in a field.
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keydown", conflict.onKeyDown);
    onCleanup(() => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keydown", conflict.onKeyDown);
    });
  });
  // The focus goes back to what opened the changes, like the file's row, once they close with it
  // inside: it would be lost to the page otherwise.
  const opener = document.activeElement;
  onCleanup(() => {
    const focused = document.activeElement;
    const lost = !focused || focused === document.body || section?.contains(focused);
    if (lost && opener instanceof HTMLElement && opener.isConnected) {
      opener.focus({ preventScroll: true });
    }
  });

  // The files a step away are loaded ahead once another one's on show, so stepping to them shows
  // them at once. Not again when the same one's patch is refetched, as an uncommitted file's is
  // whenever the working tree changes: theirs are refetched once they're opened.
  const shownKey = createMemo(() => view.shownPatch()?.key);
  createEffect(
    on(shownKey, (key) => {
      if (!key) return;
      for (const near of [previous(), next()]) {
        if (near) {
          prefetchFileDiff(
            queryClient,
            props.repositoryId,
            props.source,
            near,
            view.lists.uncounted(),
          );
        }
      }
    }),
  );

  /** Reads one side of the file on show in full: an unstaged file's new side is on disk. */
  const loadFile = (path: string) => (side: "old" | "new", oid: string) =>
    props.source.kind === "unstaged" && side === "new"
      ? fetchWorkingTreeFile(props.repositoryId, path)
      : fetchBlob(queryClient, props.repositoryId, oid);

  return (
    <section
      ref={(el) => (section = el)}
      class="flex h-full min-h-0 min-w-0 flex-col bg-bg"
      aria-label={`Changes in ${shown().file.path}`}
    >
      <FileDiffHeader
        file={shown().file}
        counts={shown().counts}
        source={props.source}
        busy={view.busy()}
        wholeFile={conflict.active() ? undefined : fileStaging.wholeFile()}
        conflict={conflict.active() ? <conflict.Controls /> : undefined}
        stagingDisabled={fileStaging.staging() || view.showsOther()}
        onStageWhole={() => void fileStaging.stageWhole()}
        previous={previous()}
        next={next()}
        onOpen={props.onOpen}
        onClose={props.onClose}
      >
        <fileEditing.Controls editable={editable()} />
      </FileDiffHeader>
      <fileEditing.Banner />
      <fileStaging.Banner />
      <Show when={conflict.active()}>
        <conflict.Banner />
      </Show>
      <Show when={view.filesError()}>{(message) => <Notice message={message()} />}</Show>

      <div class="min-h-0 flex-1">
        <Switch>
          <Match when={view.gone() || view.empty()}>
            <NoLongerChanged
              source={props.source}
              file={props.file}
              staged={view.lists.staged()}
              unstaged={view.lists.unstaged()}
              onOpen={props.onOpen}
            />
          </Match>
          <Match when={conflict.active()}>
            <conflict.View />
          </Match>
          <Match when={view.nested()}>
            <EmptyState icon={FolderGit} title="Another repository" class="h-full">
              This folder is a git repository of its own, so its files aren't listed here.
            </EmptyState>
          </Match>
          <Match when={view.binary()}>
            <EmptyState icon={FileCheck} title="Binary file" class="h-full">
              Its changes can't be shown as text.
            </EmptyState>
          </Match>
          <Match when={view.lines() === 0}>
            <EmptyState icon={FileCheck} title="No changed lines" class="h-full">
              {unchangedReason(file())}
            </EmptyState>
          </Match>
          <Match when={view.large()}>
            <EmptyState icon={FileWarning} title="Large change" class="h-full">
              <span class="flex flex-col items-center gap-3">
                {view.lines()!.toLocaleString()} lines changed. Showing them may take a moment.
                <Button onClick={view.showLarge}>Show anyway</Button>
              </span>
            </EmptyState>
          </Match>
          <Match when={true}>
            <ErrorBoundary
              fallback={(error: Error, reset) => {
                // Another file is tried afresh.
                createEffect(on(fileKey, () => reset(), { defer: true }));
                return <DiffError error={error} />;
              }}
            >
              <Suspense fallback={<LoadingState title="Loading changes…" class="h-full" />}>
                <Show
                  // Nor does an error take the editor's place: it's the last one's, from before.
                  // (`null` either way: this is keyed, so `false` would make the viewer anew.)
                  when={view.editing() ? null : view.patchQuery.error}
                  keyed
                  fallback={
                    <Show
                      // Another file's patch is only kept while it's on show: one isn't started
                      // with it.
                      when={
                        view.patch() &&
                        (view.patch()!.key === fileKey() || view.shownPatch()) &&
                        view.patch()
                      }
                      fallback={<LoadingState title="Loading changes…" class="h-full" />}
                    >
                      {(data) => {
                        onCleanup(() => {
                          fileEditing.cancelReload();
                          view.patchLeft();
                        });
                        return (
                          <PatchViewer
                            patch={data().patch}
                            fileKey={data().key}
                            cacheKey={patchCacheKey(props.source, data())}
                            diffStyle={diffStyle()}
                            loadFile={loadFile(data().file.path)}
                            onShown={() => {
                              view.patchShown(data());
                              fileEditing.onShown();
                            }}
                            editing={view.editing()}
                            onEditing={fileEditing.viewer.onEditing}
                            onEdit={fileEditing.viewer.onEdit}
                            onEditFailed={fileEditing.viewer.onEditFailed}
                            onLoadingFiles={view.setLoadingFiles}
                            onFilesError={view.setFilesError}
                            staging={fileStaging.lineStaging()}
                          />
                        );
                      }}
                    </Show>
                  }
                >
                  {(error) => <DiffError error={error} />}
                </Show>
              </Suspense>
            </ErrorBoundary>
          </Match>
        </Switch>
      </div>
    </section>
  );
}

/**
 * The bar over the changes: the file on show, its line counts, and what can be done with it, from
 * staging it whole to stepping to the files next to it. Its name stays while the next file's
 * changes load (see `FileDiffView`'s `shown`).
 */
function FileDiffHeader(props: {
  /** The file on show. */
  file: ChangedFile;
  /** How many lines changed in it, if that's known. */
  counts: LineCounts | undefined;
  source: DiffSource;
  /** Whether changes are being loaded, for long enough to say so. */
  busy: boolean;
  /** Whether the file can be staged, or unstaged, whole, and which (see `useFileStaging`). */
  wholeFile: "stage" | "unstage" | undefined;
  /** Whether it can't be right now: something's being staged, or another file's on show. */
  stagingDisabled: boolean;
  /** Resolving the file's conflicts, in place of staging it (see `useConflictResolution`). */
  conflict?: JSX.Element;
  onStageWhole: () => void;
  /** The files a step away in the list, if there are any. */
  previous: ChangedFile | undefined;
  next: ChangedFile | undefined;
  onOpen: (source: DiffSource, file: ChangedFile) => void;
  onClose: () => void;
  /** The edit mode's controls, shown for an unstaged file. */
  children?: JSX.Element;
}) {
  const { diffStyle, setDiffStyle } = useDiffStyle();
  // Made once: a prop that's JSX is made again whenever it's read.
  const conflict = children(() => props.conflict);
  const name = () => props.file.path.slice(props.file.path.lastIndexOf("/") + 1);
  const folder = () => props.file.path.slice(0, props.file.path.lastIndexOf("/") + 1);

  return (
    <header class="flex h-[42px] shrink-0 items-center gap-2 border-b border-border pr-2 pl-1.5">
      <IconButton label="Back to the history (Esc)" icon={ArrowLeft} onClick={props.onClose} />
      <FileStatusBadge status={props.file.status} />
      <span class="flex min-w-0 items-baseline gap-2">
        <span class="truncate text-[12.5px]" title={props.file.path}>
          <span class="text-faint">{folder()}</span>
          <strong class="font-[600]">{name()}</strong>
        </span>
        <Show when={props.file.origPath}>
          {(origPath) => (
            <span class="shrink-[2] truncate text-[11px] text-faint" title={origPath()}>
              from {origPath()}
            </span>
          )}
        </Show>
        <Show when={sourceLabel(props.source)}>
          {(label) => <span class="shrink-0 text-[11px] text-faint">{label()}</span>}
        </Show>
      </span>
      <Show when={props.counts}>
        {(counts) => (
          <LineStats
            additions={counts().additions}
            deletions={counts().deletions}
            class="shrink-0 text-[11px]"
          />
        )}
      </Show>
      <Show when={props.busy}>
        <LoaderCircle
          role="status"
          aria-label="Loading changes"
          size={13}
          class="shrink-0 animate-spin text-faint motion-reduce:animate-none"
        />
      </Show>
      <span class="ml-auto flex shrink-0 items-center gap-0.5">
        <Show when={props.wholeFile}>
          {(action) => (
            <>
              <Button
                icon={action() === "stage" ? Plus : Minus}
                disabled={props.stagingDisabled}
                class="h-[26px] gap-1.5 rounded-md px-2 text-[11.5px] font-[600] enabled:hover:translate-y-0"
                onClick={props.onStageWhole}
              >
                {action() === "stage" ? "Stage file" : "Unstage file"}
              </Button>
              <span class="mx-1.5 h-4 w-px bg-border" />
            </>
          )}
        </Show>
        <Show when={conflict()}>
          {conflict()}
          <span class="mx-1.5 h-4 w-px bg-border" />
        </Show>
        <Show when={props.source.kind === "unstaged"}>
          {props.children}
          <span class="mx-1.5 h-4 w-px bg-border" />
        </Show>
        <IconButton
          label="Previous file"
          icon={ChevronUp}
          disabled={!props.previous}
          onClick={() => props.previous && props.onOpen(props.source, props.previous)}
        />
        <IconButton
          label="Next file"
          icon={ChevronDown}
          disabled={!props.next}
          onClick={() => props.next && props.onOpen(props.source, props.next)}
        />
        {/* Conflicts are shown one way only, ours above theirs. */}
        <Show when={!conflict()}>
          <span class="mx-1.5 h-4 w-px bg-border" />
          <IconButton
            label="Unified"
            icon={Rows2}
            active={diffStyle() === "unified"}
            onClick={() => setDiffStyle("unified")}
          />
          <IconButton
            label="Side by side"
            icon={Columns2}
            active={diffStyle() === "split"}
            onClick={() => setDiffStyle("split")}
          />
        </Show>
      </span>
    </header>
  );
}

/**
 * Says that an uncommitted file has no changes left on its side, e.g. once it's been staged, with
 * a way to the other side's if it has some there.
 */
function NoLongerChanged(props: {
  source: DiffSource;
  file: ChangedFile;
  staged: ChangedFile[];
  unstaged: ChangedFile[];
  onOpen: (source: DiffSource, file: ChangedFile) => void;
}) {
  const other = createMemo(() => {
    const kind = props.source.kind === "unstaged" ? "staged" : "unstaged";
    const files = kind === "staged" ? props.staged : props.unstaged;
    const file = files.find((candidate) => candidate.path === props.file.path);
    return file && { source: { kind } as const, file };
  });
  return (
    <EmptyState
      icon={FileCheck}
      title={props.source.kind === "staged" ? "No staged changes" : "No unstaged changes"}
      class="h-full"
    >
      <span class="flex flex-col items-center gap-3">
        {props.source.kind === "staged"
          ? "This file's changes were unstaged or committed."
          : "This file's changes were staged or undone."}
        <Show when={other()}>
          {(side) => (
            <Button onClick={() => props.onOpen(side().source, side().file)}>
              {side().source.kind === "staged" ? "Show staged changes" : "Show unstaged changes"}
            </Button>
          )}
        </Show>
      </span>
    </EmptyState>
  );
}

function DiffError(props: { error: Error }) {
  return (
    <EmptyState icon={TriangleAlert} title="Couldn't load the changes" tone="error" class="h-full">
      {props.error.message}
    </EmptyState>
  );
}
