// What the diff view shows of one file (see `FileDiffView`): the file as its list has it now, the
// patch of its changes, and what's on show while the next one loads. Kept in one place, which
// staging the file, editing it and resolving its conflicts read from, rather than each deriving
// its own.
import { patchVersion } from "@gitto/git/patch-version";
import type { ChangedFile } from "@gitto/git/types";
import { createMemo, createSignal } from "solid-js";

import {
  countsOf,
  hasPatch,
  isKnownBinary,
  isNestedRepository,
  totalLines,
  type LineCounts,
} from "@/git/changes";
import { diffFileKey, isSameSource, isUncommitted, type DiffSource } from "@/git/diff-source";
import { isLink, summarizePatch } from "@/git/patch";
import { useDiffSourceFiles, useFilePatch, type FilePatch } from "@/git/queries/file-diff";
import { useUnsuspendedData } from "@/git/queries/unsuspended";
import { useDelayed } from "@/hooks/delayed";

/**
 * Changed lines above which a file's changes are only shown when asked: reading and parsing them
 * takes a noticeable moment, and such a change is rarely read line by line (a lockfile, say).
 */
export const LARGE_DIFF_LINES = 20_000;

/**
 * Names a file's patch in the highlighting cache. A commit's or a stash's never changes, so its
 * file's name will do; an uncommitted one's is named by what's in it too (see `patchVersion`).
 */
export function patchCacheKey(source: DiffSource, data: FilePatch): string {
  return isUncommitted(source) ? `${data.key}:${patchVersion(data.patch)}` : data.key;
}

/** The patch on show, and the file it's of. */
export interface ShownPatch {
  key: string;
  file: ChangedFile;
  patch: string;
  counts: LineCounts | undefined;
}

/** What the diff view shows of a file (see `createShownFile`). */
export type ShownFile = ReturnType<typeof createShownFile>;

/**
 * The file `props.file` of `props.source`, as the view shows it: as its list has it now, with the
 * patch of its changes, and what's on show meanwhile. Going to another file, the last one stays
 * on show, header and all, until the new one's patch is loaded and highlighted. While the file's
 * edited, it and its patch stay as they were when that started: saving the edits changes it, and
 * could take it from its list, or leave it without changes, which isn't shown in the editor.
 */
export function createShownFile(props: {
  repositoryId: () => string;
  source: () => DiffSource;
  file: () => ChangedFile;
  /** Whether the file's being edited by hand (see `useFileEditing`). */
  editing: () => boolean;
}) {
  // The same source, as one object, for as long as it is: a list's is made anew whenever it's read.
  const source = createMemo(() => props.source(), undefined, { equals: isSameSource });
  const lists = useDiffSourceFiles(props.repositoryId, source);
  const fileKey = () => diffFileKey(props.source(), props.file().path);
  const found = createMemo(() =>
    lists.files().findIndex((file) => file.path === props.file().path),
  );
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
  const liveFile = () => (isUncommitted(props.source()) && lists.files()[found()]) || props.file();
  const file = createMemo((last: ChangedFile | undefined) =>
    props.editing() && last ? last : liveFile(),
  );
  // Whether the file has gone from its list: it's been staged in full, say.
  const gone = () =>
    !props.editing() && isUncommitted(props.source()) && lists.loaded() && found() === -1;

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
  const patchEnabled = createMemo(() => !props.editing() && !gone() && fetches());
  const patchQuery = useFilePatch(props.repositoryId, source, patchFile, patchEnabled);
  // Without Suspense: an uncommitted file's patch is refetched while it's on show.
  const livePatch = useUnsuspendedData(patchQuery);
  const patch = createMemo((last: FilePatch | undefined) =>
    props.editing() && last ? last : livePatch(),
  );
  /** The patch of this file, rather than of the last one, still on show while it loads. */
  const current = () => (patch()?.key === fileKey() ? patch() : undefined);
  // What the patch says of the changes: an uncommitted file's line counts can be older than its
  // patch, and some files have none at all.
  const summaryOf = (data: FilePatch) =>
    isUncommitted(props.source()) || data.file.additions === null
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
  const conflicted = () => file().status === "conflicted";
  const nested = () => isNestedRepository(file());
  /** Whether the file's changes are shown as a patch, rather than said to be binary, say. */
  const showsPatch = () =>
    !gone() && !empty() && !conflicted() && !nested() && !binary() && lines() !== 0 && !large();

  // The file whose patch is on show. Going to another, the last one stays on show, header and
  // all, until the new one's is loaded and highlighted: they're swapped at once, without flicker.
  const [shownPatch, setShownPatch] = createSignal<ShownPatch>();
  /** The file on show: the one asked for, or the last one, while its patch is on its way. */
  const shown = () =>
    (showsPatch() && shownPatch()) || {
      key: fileKey(),
      file: file(),
      // None to count once it has no changes left on its side, nor for a conflict: git counts
      // them against one side.
      counts: gone() || empty() || conflicted() ? undefined : counts(),
    };
  // Whether the whole file is being loaded, to show more of it, and why it couldn't be.
  const [loadingFiles, setLoadingFiles] = createSignal(false);
  const [filesError, setFilesError] = createSignal<string>();
  /** Whether the file in the header is another one, still on show while this one's changes load. */
  const showsOther = () => shown().key !== fileKey();
  const busy = useDelayed(() => showsOther() || loadingFiles());
  /**
   * Whether the patch on show can be edited: an unstaged one's new side, the file on disk, if it's
   * text that's there, and not a link.
   */
  const editablePatch = () =>
    props.source().kind === "unstaged" &&
    showsPatch() &&
    !showsOther() &&
    file().status !== "deleted" &&
    !isLink(current()?.patch ?? "");

  return {
    source,
    /** The files whose changes the source has, to step through. */
    lists,
    /** Names the file in its source (see `diffFileKey`). */
    fileKey,
    /** Whether the file is still in its list. */
    listed: () => found() !== -1,
    /** The files a step away in the list, if there are any. */
    previous,
    next,
    /**
     * The file to open once this one has no changes left on its side: the next in its list, or
     * the one before the last, as one would stage them one after the other.
     */
    target: () => next() ?? previous(),
    liveFile,
    /** The file, as it's kept while it's edited. */
    file,
    editing: props.editing,
    gone,
    /** Whether the file has no changes left on its side, as its patch says. */
    empty,
    conflicted,
    nested,
    binary,
    lines,
    /** Whether the change is large, and the user hasn't asked to see it anyway. */
    large,
    showLarge: () => setShownLarge(fileKey()),
    /** The patch's query, for why it failed. */
    patchQuery,
    /** The patch loaded last: this file's, or the last one's while this one's loads. */
    patch,
    /** The patch of this file, if it's loaded. */
    current,
    summaryOf,
    showsPatch,
    /** The patch on show, if one is: the last file's, while this one's loads. */
    shownPatch,
    shown,
    showsOther,
    /** Whether changes are being loaded, for long enough to say so. */
    busy,
    filesError,
    setLoadingFiles,
    setFilesError,
    editablePatch,
    /** The patch `data` is on show. */
    patchShown: (data: FilePatch) =>
      setShownPatch({
        key: data.key,
        file: data.file,
        patch: data.patch,
        counts: summaryOf(data) ?? countsOf(data.file),
      }),
    /** No patch is on show: its viewer left the page. */
    patchLeft: () => {
      setShownPatch(undefined);
      setLoadingFiles(false);
      setFilesError(undefined);
    },
  };
}
