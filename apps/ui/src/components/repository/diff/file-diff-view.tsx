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
  createSignal,
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
import { EmptyState } from "@/components/ui/empty-state";
import { FileStatusBadge } from "@/components/ui/file-status-badge";
import { LineStats } from "@/components/ui/line-stats";
import { Notice } from "@/components/ui/notice";
import {
  countsOf,
  hasPatch,
  isKnownBinary,
  isNestedRepository,
  sourceLabel,
  totalLines,
  unchangedReason,
  type LineCounts,
} from "@/git/changes";
import { diffFileKey, isSameSource, isUncommitted, type DiffSource } from "@/git/diff-source";
import { isLink, patchVersion, summarizePatch } from "@/git/patch";
import { fetchBlob } from "@/git/queries/diff";
import {
  fetchFilePatch,
  fetchWorkingTreeFile,
  useDiffSourceFiles,
  useFilePatch,
  type FilePatch,
} from "@/git/queries/file-diff";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useDelayed } from "@/hooks/delayed";
import { useDiffStyle } from "@/hooks/diff-style";
import { isTyping } from "@/lib/typing";

import { prefetchConflict, useConflictResolution } from "./conflict-resolution";
import { useFileEditing } from "./file-editing";
import { useFileStaging } from "./file-staging";
import type * as ViewerModule from "./patch-viewer";

let viewerModule: Promise<typeof ViewerModule> | undefined;
/** The viewer's module, loaded once, the first time it's needed: it brings Shiki. */
const loadViewer = () => (viewerModule ??= import("./patch-viewer"));
const PatchViewer = lazy(loadViewer);

/**
 * Changed lines above which a file's changes are only shown when asked: reading and parsing them
 * takes a noticeable moment, and such a change is rarely read line by line (a lockfile, say).
 */
const LARGE_DIFF_LINES = 20_000;

/**
 * Changed lines above which a file's changes aren't loaded ahead (see `prefetchFileDiff`): parsing
 * them on the main thread starts to take a while, for a file that may not be opened after all.
 */
const PREFETCH_DIFF_LINES = 5_000;

/**
 * Names a file's patch in the highlighting cache. A commit's or a stash's never changes, so its
 * file's name will do; an uncommitted one's is named by what's in it too (see `patchVersion`).
 */
function patchCacheKey(source: DiffSource, data: FilePatch): string {
  return isUncommitted(source) ? `${data.key}:${patchVersion(data.patch)}` : data.key;
}

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
  // The same source, as one object, for as long as it is: a list's is made anew whenever it's read.
  const source = createMemo(() => props.source, undefined, { equals: isSameSource });
  const lists = useDiffSourceFiles(() => props.repositoryId, source);
  const fileKey = () => diffFileKey(props.source, props.file.path);
  const found = createMemo(() => lists.files().findIndex((file) => file.path === props.file.path));
  // Where the file was in its list, after it's gone from it: the files around it stay a step away.
  // Only this file's place: another one opened that isn't in its list has none.
  const lastPlace = createMemo(
    (last: { key: string; index: number }) =>
      found() !== -1
        ? { key: fileKey(), index: found() }
        : last.key === fileKey()
          ? last
          : { key: fileKey(), index: -1 },
    { key: "", index: -1 },
  );
  const place = () => lastPlace().index;
  const previous = () => (place() > 0 ? lists.files()[place() - 1] : undefined);
  const next = () =>
    place() === -1 ? undefined : lists.files()[found() === -1 ? place() : place() + 1];

  /**
   * The file as its list has it now: uncommitted changes change on disk, and are listed anew. As
   * it was opened if it's gone from the list.
   */
  const liveFile = () => (isUncommitted(props.source) && lists.files()[found()]) || props.file;
  const fileEditing = useFileEditing({
    repositoryId: () => props.repositoryId,
    path: () => props.file.path,
  });
  const editing = fileEditing.editing;
  props.onGuard?.(fileEditing.leave);
  onCleanup(() => props.onGuard?.(undefined));
  // While it's edited, the file stays as it was when that started: saving the edits changes it,
  // and could take it from its list, or leave it without changes, which isn't shown in the editor.
  const file = createMemo((last: ChangedFile | undefined) =>
    editing() && last ? last : liveFile(),
  );
  // Whether the file has gone from its list: it's been staged in full, say.
  const gone = () => !editing() && isUncommitted(props.source) && lists.loaded() && found() === -1;

  // The large file the user asked to see anyway.
  const [shownLarge, setShownLarge] = createSignal<string>();
  const fetches = () => {
    const counts = countsOf(file());
    if (!hasPatch(file(), lists.uncounted())) return false;
    return !counts || totalLines(counts) <= LARGE_DIFF_LINES || shownLarge() === fileKey();
  };
  // What the patch's query is of, changing only when that does: an uncommitted file's list is
  // replaced whenever anything in the working tree changes, and the query's options are set again
  // (and its resource refetched) whenever they're read anew.
  const patchFile = createMemo(file, undefined, {
    equals: (a, b) => a.path === b.path && a.origPath === b.origPath && a.status === b.status,
  });
  // Not refetched while it's edited, as each save would: the view keeps the patch editing started
  // from (see `file`), and gets the last one once editing stops.
  const patchEnabled = createMemo(() => !editing() && !gone() && fetches());
  const patchQuery = useFilePatch(() => props.repositoryId, source, patchFile, patchEnabled);
  // Without Suspense: an uncommitted file's patch is refetched while it's on show.
  const livePatch = useUnsuspendedData(patchQuery);
  const patch = createMemo((last: FilePatch | undefined) =>
    editing() && last ? last : livePatch(),
  );
  /** The patch of this file, rather than of the last one, still on show while it loads. */
  const current = () => (patch()?.key === fileKey() ? patch() : undefined);
  // What the patch says of the changes: an uncommitted file's line counts can be older than its
  // patch, and some files have none at all.
  const summaryOf = (data: FilePatch) =>
    isUncommitted(props.source) || data.file.additions === null
      ? summarizePatch(data.patch)
      : undefined;
  const summary = createMemo(() => {
    const data = current();
    return data && summaryOf(data);
  });
  const empty = () => current()?.patch === "";
  const binary = () => isKnownBinary(file(), lists.uncounted()) || summary()?.binary === true;
  /** How many lines changed; `undefined` until the patch says, for a file without counts. */
  const counts = () => (summary() && !summary()!.binary ? summary() : countsOf(file()));
  const lines = () => (counts() ? totalLines(counts()!) : undefined);
  const large = () => (lines() ?? 0) > LARGE_DIFF_LINES && shownLarge() !== fileKey();
  const showsPatch = () =>
    !gone() &&
    !empty() &&
    file().status !== "conflicted" &&
    !isNestedRepository(file()) &&
    !binary() &&
    lines() !== 0 &&
    !large();

  // The file whose patch is on show. Going to another, the last one stays on show, header and
  // all, until the new one's is loaded and highlighted: they're swapped at once, without flicker.
  const [shownPatch, setShownPatch] = createSignal<{
    key: string;
    file: ChangedFile;
    patch: string;
    counts: LineCounts | undefined;
  }>();
  /** The file on show: the one asked for, or the last one, while its patch is on its way. */
  const shown = () =>
    (showsPatch() && shownPatch()) || {
      key: fileKey(),
      file: file(),
      // None to count once it has no changes left on its side, nor for a conflict: git counts
      // them against one side.
      counts: gone() || empty() || conflict.active() ? undefined : counts(),
    };
  // Whether the whole file is being loaded, to show more of it, and why it couldn't be.
  const [loadingFiles, setLoadingFiles] = createSignal(false);
  const [filesError, setFilesError] = createSignal<string>();
  const busy = useDelayed(() => shown().key !== fileKey() || loadingFiles());
  /**
   * Whether the file on show can be edited: an unstaged one's new side, the file on disk, if it's
   * text that's there, and not a link.
   */
  const editable = () =>
    (props.source.kind === "unstaged" &&
      showsPatch() &&
      shown().key === fileKey() &&
      file().status !== "deleted" &&
      !isLink(current()?.patch ?? "")) ||
    conflict.editable();

  /** Whether the file in the header is another one, still on show while this one's changes load. */
  const showsOther = () => shown().key !== fileKey();
  const fileStaging = useFileStaging({
    repositoryId: () => props.repositoryId,
    source: () => props.source,
    fileKey,
    file,
    liveFile,
    listed: () => found() !== -1,
    current,
    shownPatch: () => shownPatch()?.patch,
    showsOther,
    editing,
    showsPatch,
    target: () => next() ?? previous(),
    leave: fileEditing.leave,
    open: (target) => props.onOpen(props.source, target),
  });
  const conflict = useConflictResolution({
    repositoryId: () => props.repositoryId,
    file,
    fileKey,
    files: lists.files,
    editing,
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
  const shownKey = createMemo(() => shownPatch()?.key);
  createEffect(
    on(shownKey, (key) => {
      if (!key) return;
      for (const near of [previous(), next()]) {
        if (near) {
          prefetchFileDiff(queryClient, props.repositoryId, props.source, near, lists.uncounted());
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
        busy={busy()}
        wholeFile={conflict.active() ? undefined : fileStaging.wholeFile()}
        conflict={conflict.active() ? <conflict.Controls /> : undefined}
        stagingDisabled={fileStaging.staging() || showsOther()}
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
      <Show when={filesError()}>{(message) => <Notice message={message()} />}</Show>

      <div class="min-h-0 flex-1">
        <Switch>
          <Match when={gone() || empty()}>
            <NoLongerChanged
              source={props.source}
              file={props.file}
              staged={lists.staged()}
              unstaged={lists.unstaged()}
              onOpen={props.onOpen}
            />
          </Match>
          <Match when={conflict.active()}>
            <conflict.View />
          </Match>
          <Match when={isNestedRepository(file())}>
            <EmptyState icon={FolderGit} title="Another repository" class="h-full">
              This folder is a git repository of its own, so its files aren't listed here.
            </EmptyState>
          </Match>
          <Match when={binary()}>
            <EmptyState icon={FileCheck} title="Binary file" class="h-full">
              Its changes can't be shown as text.
            </EmptyState>
          </Match>
          <Match when={lines() === 0}>
            <EmptyState icon={FileCheck} title="No changed lines" class="h-full">
              {unchangedReason(file())}
            </EmptyState>
          </Match>
          <Match when={large()}>
            <EmptyState icon={FileWarning} title="Large change" class="h-full">
              <span class="flex flex-col items-center gap-3">
                {lines()!.toLocaleString()} lines changed. Showing them may take a moment.
                <Button onClick={() => setShownLarge(fileKey())}>Show anyway</Button>
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
              <Suspense fallback={<Loading />}>
                <Show
                  // Nor does an error take the editor's place: it's the last one's, from before.
                  // (`null` either way: this is keyed, so `false` would make the viewer anew.)
                  when={editing() ? null : patchQuery.error}
                  keyed
                  fallback={
                    <Show
                      // Another file's patch is only kept while it's on show: one isn't started
                      // with it.
                      when={patch() && (patch()!.key === fileKey() || shownPatch()) && patch()}
                      fallback={<Loading />}
                    >
                      {(data) => {
                        onCleanup(() => {
                          fileEditing.cancelReload();
                          setShownPatch(undefined);
                          setLoadingFiles(false);
                          setFilesError(undefined);
                        });
                        return (
                          <PatchViewer
                            patch={data().patch}
                            fileKey={data().key}
                            cacheKey={patchCacheKey(props.source, data())}
                            diffStyle={diffStyle()}
                            loadFile={loadFile(data().file.path)}
                            onShown={() => {
                              setShownPatch({
                                key: data().key,
                                file: data().file,
                                patch: data().patch,
                                counts: summaryOf(data()) ?? countsOf(data().file),
                              });
                              fileEditing.onShown();
                            }}
                            editing={editing()}
                            onEditing={fileEditing.viewer.onEditing}
                            onEdit={fileEditing.viewer.onEdit}
                            onEditFailed={fileEditing.viewer.onEditFailed}
                            onLoadingFiles={setLoadingFiles}
                            onFilesError={setFilesError}
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

/** Shown while the changes load, unless they're quick about it. */
function Loading() {
  const shown = useDelayed(() => true, 150);
  return (
    <Show when={shown()}>
      <EmptyState icon={LoaderCircle} loading title="Loading changes…" class="h-full" />
    </Show>
  );
}
