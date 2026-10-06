// What the viewer runs on, shared by every one of it on the page: the workers that highlight, the
// library's diff with some of what it keeps to itself in reach, and parsing patches and reading
// whole files into diffs. Loaded with the viewer (see `FileDiffView`), as it brings Shiki.
import {
  getSharedHighlighter,
  hydratePartialDiff,
  parseDiffFromFile,
  parsePatchFiles,
  VirtualizedFileDiff,
  type FileDiffLoadedFiles,
  type FileDiffMetadata,
  type HunkExpansionRegion,
  type PostRenderPhase,
} from "@pierre/diffs";
import { getOrCreateWorkerPoolSingleton, type WorkerPoolManager } from "@pierre/diffs/worker";

import type { HunkButton } from "./line-staging";
import { fitToPatch, hasMatchingEnds, type Side } from "./patch-files";

/** One side of a file, read in full: with its version if it's the working tree's. */
export type LoadedFile = string | { contents: string; version: string };

/** Highlighting runs in workers; one diff is on show at a time, so a couple is plenty. */
const WORKERS = 2;

/**
 * How many diffs the workers' manager keeps highlighted, in the main thread, for a quick return
 * to one shown lately, or prepared ahead (see `preparePatch`): the files a step away. The
 * library's default is a hundred, whatever their size, and a highlighted line takes 1-5KB: 40MB
 * for a file of 30,000 lines read in full to show more of it, 200MB for a change of 25,000 lines.
 * So the long ones are dropped once their view closes too (see `KEPT_LINES`).
 */
const CACHED_DIFFS = 16;

/**
 * Lines (of both sides) from which a diff's highlighting is dropped once the view that showed it
 * closes, rather than kept among the `CACHED_DIFFS`: a whole file read to show more lines, edits,
 * or a long patch. Shorter ones highlight again in well under the time the view waits for that
 * (see `HIGHLIGHT_WAIT_MS`), so they're kept for a quick return.
 */
const KEPT_LINES = 1000;

/** The library's themes, dark and light, which the viewer draws with. */
export const THEMES = { dark: "pierre-dark", light: "pierre-light" } as const;

/**
 * The app's background and colours for added and removed lines, in place of the theme's, and the
 * focus ring on the buttons that show more lines (see `focusableExpandButtons`).
 */
export const APP_CSS = `:host {
  --diffs-dark-bg: var(--bg);
  --diffs-light-bg: var(--bg);
  --diffs-addition-color-override: var(--mint);
  --diffs-deletion-color-override: var(--coral);
}
[data-expand-button]:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: -2px;
}`;

let pool: WorkerPoolManager | undefined;

/** The workers that highlight, started the first time they're needed. */
export function workerPool(): WorkerPoolManager {
  if (pool) return pool;
  // The main thread has a highlighter of its own, which only draws plain text in the viewer (and
  // edits, in the library's editor): Shiki's JavaScript engine does, without loading and compiling
  // the WebAssembly one there. The first one asked for is the one that's shared, so it's this.
  void getSharedHighlighter({
    themes: [THEMES.dark, THEMES.light],
    langs: ["text"],
    preferredHighlighter: "shiki-js",
  }).catch((error: unknown) => console.error("Couldn't start the highlighter", error));
  pool = getOrCreateWorkerPoolSingleton({
    poolOptions: {
      workerFactory: () =>
        new Worker(new URL("@pierre/diffs/worker/worker.js", import.meta.url), { type: "module" }),
      poolSize: WORKERS,
      totalASTLRUCacheSize: CACHED_DIFFS,
    },
    // Oniguruma, as in VS Code, rather than Shiki's JavaScript engine: about twice as fast on a
    // diff of thousands of lines (1.5s against 3s for 6,000), and no slower on small ones. With
    // each token's place in its line, which editing a file needs: without, starting to edit one
    // highlights it all again on the main thread (1.9s for 2,500 lines, the window frozen), for
    // about 15% longer highlighting here (0.69s against 0.59s for 4,500 lines).
    highlighterOptions: {
      theme: THEMES,
      preferredHighlighter: "shiki-wasm",
      useTokenTransformer: true,
    },
  });
  return pool;
}

/**
 * How long a diff waits for its highlighting before it's shown without, until it's highlighted:
 * long enough for most, which then show highlighted at once rather than flicker from plain text,
 * short enough not to hold up one of thousands of lines, which takes a second or more.
 */
export const HIGHLIGHT_WAIT_MS = 300;

/**
 * How long expanding the lines around the changes waits for the whole file to be highlighted. The
 * viewer gives it 300ms by itself, then shows the whole diff as plain text until it's done, which
 * flickers for a file of a thousand lines or so.
 */
export const EXPAND_HIGHLIGHT_WAIT_MS = 1500;

/** The library's virtualized diff, with some of what it keeps to itself in reach. */
export class ViewerFileDiff extends VirtualizedFileDiff<HunkButton, undefined> {
  /**
   * The lines shown around each hunk: they're kept by hunk across diffs, so another file's would
   * show the last one's (see `PatchViewer`'s `show`).
   */
  get expanded(): ReadonlyMap<number, HunkExpansionRegion> {
    return this.hunksRenderer.getExpandedHunksMap();
  }

  set expanded(expanded: Map<number, HunkExpansionRegion>) {
    this.hunksRenderer.setExpandedHunksMap(expanded);
  }

  /**
   * Names the diff being edited in the highlighting cache, which the library leaves unnamed: once
   * editing stops, a diff with edits is highlighted again, in the workers only if it has a name,
   * on the main thread otherwise (1.7s for 2,500 lines, the window frozen). Returns the diff, if
   * one's being edited, named as it was if it already had a name.
   */
  nameEditedDiff(cacheKey: string): FileDiffMetadata | undefined {
    const diff = this.getLatestDiff();
    if (diff && diff.cacheKey == null) diff.cacheKey = cacheKey;
    return diff;
  }
}

/** Tells apart the diffs left by editing, in the highlighting cache. */
let editedDiffs = 0;

/** The number of the next diff left by editing, to name it in the highlighting cache. */
export function nextEditedDiff(): number {
  return ++editedDiffs;
}

/**
 * Drops a patch of a file that's been replaced by a newer one from the highlighting cache, which
 * has room for `CACHED_DIFFS`: an uncommitted file can be saved many times while it's on show.
 */
export function forget(diff: FileDiffMetadata) {
  if (!diff.cacheKey) return;
  workerPool().evictDiffFromCache(diff.cacheKey);
  const partial = diff.cacheKey.replace(/:hydrated$/, "");
  if (partial !== diff.cacheKey) workerPool().evictDiffFromCache(partial);
}

/**
 * Makes the buttons that show more lines reachable with the keyboard, after each render, as the
 * library draws them as `div`s that take a click: Enter or Space clicks the one with the focus.
 */
export function focusableExpandButtons(container: HTMLElement, _: unknown, phase: PostRenderPhase) {
  const root = container.shadowRoot;
  if (!root || phase === "unmount") return;
  for (const button of root.querySelectorAll<HTMLElement>("[data-expand-button]:not([tabindex])")) {
    button.tabIndex = 0;
    button.setAttribute("aria-label", "Show unmodified lines");
  }
  if (phase === "mount") root.addEventListener("keydown", clickExpandButton);
}

function clickExpandButton(event: Event) {
  const { key, target } = event as KeyboardEvent;
  if (key !== "Enter" && key !== " ") return;
  if (!(target instanceof HTMLElement) || !target.hasAttribute("data-expand-button")) return;
  event.preventDefault();
  target.click();
}

/** Resolves once `diff` is highlighted, or `ms` later; never rejects. */
export function highlighted(diff: FileDiffMetadata, ms: number): Promise<void> {
  return Promise.race([
    // Shown as plain text if it can't be highlighted.
    workerPool()
      .primeDiffHighlightCache(diff)
      .catch(() => undefined),
    new Promise<void>((resolve) => setTimeout(resolve, ms)),
  ]);
}

/**
 * The diffs one view had highlighted, to drop the long ones from the workers' manager's cache once
 * the view closes (see `CACHED_DIFFS`, `KEPT_LINES`): the whole files read to show more lines or to
 * edit them, the edits, and long patches, which it would otherwise keep for as long as the app runs.
 */
export class ViewHighlights {
  private readonly diffs = new Map<string, FileDiffMetadata>();

  /** Remembers `diff` as one the view had highlighted, if it's named in the cache. */
  remember(diff: FileDiffMetadata | undefined): void {
    if (diff?.cacheKey) this.diffs.set(diff.cacheKey, diff);
  }

  /** `highlighted`, remembering the diff. */
  highlight(diff: FileDiffMetadata, ms: number): Promise<void> {
    this.remember(diff);
    return highlighted(diff, ms);
  }

  /** `loadFiles`, remembering the diff filled in with the files. */
  async loadWhole(
    diff: FileDiffMetadata,
    loadFile: (side: Side, oid: string) => Promise<LoadedFile>,
  ): ReturnType<typeof loadFiles> {
    const loaded = await loadFiles(diff, loadFile);
    this.remember(loaded.hydrated);
    return loaded;
  }

  /** Drops the long diffs from the cache, as the view closes; the shorter ones stay. */
  drop(): void {
    for (const [key, diff] of this.diffs) {
      if (diff.additionLines.length + diff.deletionLines.length >= KEPT_LINES) {
        workerPool().evictDiffFromCache(key);
      }
    }
    this.diffs.clear();
  }
}

/** Patches parsed lately, by their cache key, so one prepared ahead isn't parsed again. */
const parsedPatches = new Map<string, { patch: string; diff: FileDiffMetadata }>();
const PARSED_PATCHES_KEPT = 16;

/** `parsePatch`, of a patch that may have been parsed lately. */
export function parsed(patch: string, cacheKey: string): FileDiffMetadata {
  const kept = parsedPatches.get(cacheKey);
  if (kept?.patch === patch) return kept.diff;
  const diff = parsePatch(patch, cacheKey);
  parsedPatches.delete(cacheKey);
  parsedPatches.set(cacheKey, { patch, diff });
  // The oldest go first: a map keeps the order things were put in.
  for (const key of parsedPatches.keys()) {
    if (parsedPatches.size <= PARSED_PATCHES_KEPT) break;
    parsedPatches.delete(key);
  }
  return diff;
}

/**
 * Parses and highlights a patch ahead of showing it, with the same `cacheKey`, so it shows at once.
 * Rejects if it can't be parsed.
 */
export async function preparePatch(patch: string, cacheKey: string): Promise<void> {
  await workerPool().primeDiffHighlightCache(parsed(patch, cacheKey));
}

/** The one file in `patch`. */
export function parsePatch(patch: string, cacheKey: string): FileDiffMetadata {
  const [file, other] = parsePatchFiles(patch, cacheKey)[0]?.files ?? [];
  if (!file) throw new Error("The patch has no file in it.");
  // A file that changed type, e.g. from a symbolic link, is a deletion and an addition to git,
  // each with the whole of its side: shown as one change, from the one's contents to the other's.
  if (file.type === "deleted" && other?.type === "new") {
    return parseDiffFromFile(
      { name: file.name, contents: file.deletionLines.join(""), cacheKey: `${cacheKey}:old` },
      { name: other.name, contents: other.additionLines.join(""), cacheKey: `${cacheKey}:new` },
    );
  }
  return file;
}

/**
 * Both sides of `diff` in full, read by the object names on its `index` line, and a copy of `diff`
 * filled in with them. Rejects if they aren't the files the patch is of, as a file in the working
 * tree can have changed since.
 */
export async function loadFiles(
  diff: FileDiffMetadata,
  loadFile: (side: Side, oid: string) => Promise<LoadedFile>,
): Promise<{ files: FileDiffLoadedFiles; hydrated: FileDiffMetadata; version?: string }> {
  const { prevObjectId, newObjectId } = diff;
  if (!prevObjectId || !newObjectId) throw new Error("The patch doesn't name its files.");
  const [old, read] = await Promise.all([
    loadFile("old", prevObjectId),
    loadFile("new", newObjectId),
  ]);
  const before = typeof old === "string" ? old : old.contents;
  const after = typeof read === "string" ? read : read.contents;
  // Named in the highlighting cache by the patch's object names: the working tree's side is by
  // what git would store it as, so another version of the file has another name too.
  const files = {
    oldFile: {
      name: diff.prevName ?? diff.name,
      contents: fitToPatch(diff, "old", before),
      cacheKey: prevObjectId,
    },
    newFile: { name: diff.name, contents: fitToPatch(diff, "new", after), cacheKey: newObjectId },
  };
  const hydrated = hydratePartialDiff("clone", diff, files);
  if (!hasMatchingEnds(hydrated)) {
    throw new Error("The file has changed since its changes were loaded. Try again in a moment.");
  }
  // Highlighted before they're handed over (see `EXPAND_HIGHLIGHT_WAIT_MS`): the copy, filled in
  // as the viewer will, has the same key in the highlighting cache.
  await highlighted(hydrated, EXPAND_HIGHLIGHT_WAIT_MS);
  return { files, hydrated, version: typeof read === "string" ? undefined : read.version };
}
