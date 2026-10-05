import type { ChangedFile } from "@gitto/git/types";
import { useQueryClient, type QueryClient } from "@tanstack/solid-query";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import ChevronDown from "lucide-solid/icons/chevron-down";
import ChevronUp from "lucide-solid/icons/chevron-up";
import Columns2 from "lucide-solid/icons/columns-2";
import FileCheck from "lucide-solid/icons/file-check";
import FileWarning from "lucide-solid/icons/file-exclamation-point";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import Rows2 from "lucide-solid/icons/rows-2";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import {
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
} from "solid-js";

import { FileStatusBadge } from "@/components/repository/details/changed-file-list";
import { Button, IconButton } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { LineStats } from "@/components/ui/line-stats";
import {
  fetchBlob,
  fetchCommitFilePatch,
  useCommitFilePatch,
  useCommitFiles,
} from "@/git/queries/diff";
import { useDelayed } from "@/hooks/delayed";
import { useDiffStyle } from "@/hooks/diff-style";

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

/** Names a file's patch in the highlighting cache, by its commit and path. */
const patchCacheKey = (sha: string, path: string) => `${sha}:${path}`;

/**
 * Loads and highlights a file's changes ahead of opening them, e.g. when it's pointed at, so they
 * show at once; the first time, that starts the viewer too. Not for one that's binary, or large.
 */
export function prefetchFileDiff(
  client: QueryClient,
  repositoryId: string,
  sha: string,
  file: ChangedFile,
): void {
  if (file.additions === null) return;
  const lines = file.additions + (file.deletions ?? 0);
  if (lines === 0 || lines > PREFETCH_DIFF_LINES) return;
  void Promise.all([fetchCommitFilePatch(client, repositoryId, sha, file), loadViewer()])
    .then(([data, viewer]) =>
      viewer.preparePatch(data.patch, patchCacheKey(data.sha, data.file.path)),
    )
    // Only ahead of time: opening it says what went wrong.
    .catch(() => undefined);
}

/**
 * The changes in one of a commit's files, in place of the history. Esc, or the back button, goes
 * back to the history; the commit's other files are a step away.
 */
export function FileDiffView(props: {
  repositoryId: string;
  sha: string;
  file: ChangedFile;
  /** Shows another of the commit's files. */
  onOpen: (file: ChangedFile) => void;
  onClose: () => void;
}) {
  const { diffStyle, setDiffStyle } = useDiffStyle();
  const queryClient = useQueryClient();
  const commitFiles = useCommitFiles(
    () => props.repositoryId,
    () => props.sha,
  );
  const index = createMemo(() =>
    commitFiles.files().findIndex((file) => file.path === props.file.path),
  );
  const previous = () => (index() > 0 ? commitFiles.files()[index() - 1] : undefined);
  const next = () => (index() === -1 ? undefined : commitFiles.files()[index() + 1]);

  const lines = () => (props.file.additions ?? 0) + (props.file.deletions ?? 0);
  const binary = () => props.file.additions === null;
  // The large file the user asked to see anyway, by its path.
  const [shownLarge, setShownLarge] = createSignal<string>();
  const large = () => lines() > LARGE_DIFF_LINES && shownLarge() !== props.file.path;
  const showsPatch = () => !binary() && lines() > 0 && !large();
  const patch = useCommitFilePatch(
    () => props.repositoryId,
    () => props.sha,
    () => props.file,
    showsPatch,
  );
  // The file whose patch is on show. Going to another, the last one stays on show, header and
  // all, until the new one's is loaded and highlighted: they're swapped at once, without flicker.
  const [shownPatchFile, setShownPatchFile] = createSignal<ChangedFile>();
  /** The file on show: the one asked for, or the last one, while its patch is on its way. */
  const shown = () => (showsPatch() && shownPatchFile()) || props.file;
  // Whether the whole file is being loaded, to show more of it, and why it couldn't be.
  const [loadingFiles, setLoadingFiles] = createSignal(false);
  const [filesError, setFilesError] = createSignal<string>();
  const busy = useDelayed(() => shown().path !== props.file.path || loadingFiles());

  let section: HTMLElement | undefined;
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented || isEditable(event.target)) return;
    event.preventDefault();
    props.onClose();
  };
  onMount(() => {
    // On the window, so it hears Esc after a popover, menu or dialog has (on the document): one
    // that closes on it marks it handled, and the changes stay open.
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
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

  // The files a step away are loaded ahead once this one's on show, so stepping to them shows them
  // at once.
  createEffect(
    on(shownPatchFile, (file) => {
      if (!file) return;
      for (const near of [previous(), next()]) {
        if (near) prefetchFileDiff(queryClient, props.repositoryId, props.sha, near);
      }
    }),
  );

  const name = () => shown().path.slice(shown().path.lastIndexOf("/") + 1);
  const folder = () => shown().path.slice(0, shown().path.lastIndexOf("/") + 1);

  return (
    <section
      ref={(el) => (section = el)}
      class="flex h-full min-h-0 min-w-0 flex-col bg-bg"
      aria-label={`Changes in ${shown().path}`}
    >
      <header class="flex h-[42px] shrink-0 items-center gap-2 border-b border-border pr-2 pl-1.5">
        <IconButton label="Back to the history (Esc)" icon={ArrowLeft} onClick={props.onClose} />
        <FileStatusBadge status={shown().status} />
        <span class="flex min-w-0 items-baseline gap-2">
          <span class="truncate text-[12.5px]" title={shown().path}>
            <span class="text-faint">{folder()}</span>
            <strong class="font-[600]">{name()}</strong>
          </span>
          <Show when={shown().origPath}>
            {(origPath) => (
              <span class="shrink-[2] truncate text-[11px] text-faint" title={origPath()}>
                from {origPath()}
              </span>
            )}
          </Show>
        </span>
        <Show when={shown().additions !== null}>
          <LineStats
            additions={shown().additions ?? 0}
            deletions={shown().deletions ?? 0}
            class="shrink-0 text-[11px]"
          />
        </Show>
        <Show when={busy()}>
          <LoaderCircle
            role="status"
            aria-label="Loading changes"
            size={13}
            class="shrink-0 animate-spin text-faint motion-reduce:animate-none"
          />
        </Show>
        <span class="ml-auto flex shrink-0 items-center gap-0.5">
          <IconButton
            label="Previous file"
            icon={ChevronUp}
            disabled={!previous()}
            onClick={() => previous() && props.onOpen(previous()!)}
          />
          <IconButton
            label="Next file"
            icon={ChevronDown}
            disabled={!next()}
            onClick={() => next() && props.onOpen(next()!)}
          />
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
        </span>
      </header>
      <Show when={filesError()}>
        {(message) => (
          <p
            role="status"
            class="m-0 flex shrink-0 items-center gap-1.5 border-b border-border px-3 py-1.5 text-[11.5px] text-muted"
          >
            <FileWarning size={13} class="shrink-0 text-amber" />
            {message()}
          </p>
        )}
      </Show>

      <div class="min-h-0 flex-1">
        <Switch>
          <Match when={binary()}>
            <EmptyState icon={FileCheck} title="Binary file" class="h-full">
              Its changes can't be shown as text.
            </EmptyState>
          </Match>
          <Match when={lines() === 0}>
            <EmptyState icon={FileCheck} title="No changed lines" class="h-full">
              {unchangedReason(props.file)}
            </EmptyState>
          </Match>
          <Match when={large()}>
            <EmptyState icon={FileWarning} title="Large change" class="h-full">
              <span class="flex flex-col items-center gap-3">
                {lines().toLocaleString()} lines changed. Showing them may take a moment.
                <Button onClick={() => setShownLarge(props.file.path)}>Show anyway</Button>
              </span>
            </EmptyState>
          </Match>
          <Match when={true}>
            <ErrorBoundary
              fallback={(error: Error, reset) => {
                // Another file is tried afresh.
                createEffect(
                  on(
                    () => props.file.path,
                    () => reset(),
                    { defer: true },
                  ),
                );
                return <DiffError error={error} />;
              }}
            >
              <Suspense fallback={<Loading />}>
                <Show
                  when={patch.error}
                  keyed
                  fallback={
                    <Show
                      // Another file's patch is only kept while it's on show: one isn't started
                      // with it.
                      when={
                        patch.data &&
                        (patch.data.file.path === props.file.path || shownPatchFile()) &&
                        patch.data
                      }
                      fallback={<Loading />}
                    >
                      {(data) => {
                        onCleanup(() => {
                          setShownPatchFile(undefined);
                          setLoadingFiles(false);
                          setFilesError(undefined);
                        });
                        return (
                          <PatchViewer
                            patch={data().patch}
                            cacheKey={patchCacheKey(data().sha, data().file.path)}
                            diffStyle={diffStyle()}
                            loadFile={(oid) => fetchBlob(queryClient, props.repositoryId, oid)}
                            onShown={() => setShownPatchFile(data().file)}
                            onLoadingFiles={setLoadingFiles}
                            onFilesError={setFilesError}
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

/** Why a file without binary contents has no changed lines. */
function unchangedReason(file: ChangedFile): string {
  if (file.origPath) return `Renamed from ${file.origPath}, with the same contents.`;
  if (file.status === "added") return "An empty file was added.";
  if (file.status === "deleted") return "An empty file was deleted.";
  return "Only the file's mode changed.";
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

/** Whether a key pressed in `target` is typing, e.g. in the commit message. */
function isEditable(target: EventTarget | null) {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}
