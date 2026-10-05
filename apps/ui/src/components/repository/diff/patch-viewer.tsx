// The diff viewer, from @pierre/diffs. Loaded on its own (see `FileDiffView`), as it brings Shiki.
import {
  hydratePartialDiff,
  parseDiffFromFile,
  parsePatchFiles,
  VirtualizedFileDiff,
  Virtualizer,
  type FileDiffLoadedFiles,
  type FileDiffMetadata,
  type FileDiffOptions,
} from "@pierre/diffs";
import { getOrCreateWorkerPoolSingleton } from "@pierre/diffs/worker";
import { createEffect, createMemo, on, onCleanup, untrack } from "solid-js";

import { useConnected } from "@/components/ui/virtual-list";
import type { DiffStyle } from "@/hooks/diff-style";
import { useTheme } from "@/hooks/theme";

/** Highlighting runs in workers; one diff is on show at a time, so a couple is plenty. */
const WORKERS = 2;

const THEMES = { dark: "pierre-dark", light: "pierre-light" } as const;

/** The app's background and colours for added and removed lines, in place of the theme's. */
const APP_COLORS = `:host {
  --diffs-dark-bg: var(--bg);
  --diffs-light-bg: var(--bg);
  --diffs-addition-color-override: var(--mint);
  --diffs-deletion-color-override: var(--coral);
}`;

const workerPool = () =>
  getOrCreateWorkerPoolSingleton({
    poolOptions: {
      workerFactory: () =>
        new Worker(new URL("@pierre/diffs/worker/worker.js", import.meta.url), { type: "module" }),
      poolSize: WORKERS,
    },
    // Oniguruma, as in VS Code, rather than Shiki's JavaScript engine: about twice as fast on a
    // diff of thousands of lines (1.5s against 3s for 6,000), and no slower on small ones.
    highlighterOptions: { theme: THEMES, preferredHighlighter: "shiki-wasm" },
  });

/**
 * How long a diff waits for its highlighting before it's shown without, until it's highlighted:
 * long enough for most, which then show highlighted at once rather than flicker from plain text,
 * short enough not to hold up one of thousands of lines, which takes a second or more.
 */
const HIGHLIGHT_WAIT_MS = 300;

/**
 * How long expanding the lines around the changes waits for the whole file to be highlighted. The
 * viewer gives it 300ms by itself, then shows the whole diff as plain text until it's done, which
 * flickers for a file of a thousand lines or so.
 */
const EXPAND_HIGHLIGHT_WAIT_MS = 1500;

/**
 * One file's patch, as git gave it, with only the lines on screen rendered. Only the changes and
 * the lines around them are in the patch; the rest of the file is read with `loadFile`, by the
 * object names on the patch's `index` line, once the lines between are asked for.
 *
 * Given another patch, it keeps the last one on show until the new one is highlighted (see
 * `HIGHLIGHT_WAIT_MS`), then shows it in its place and calls `onShown`. What it says of loading
 * the whole file is of the patch on show.
 */
export default function PatchViewer(props: {
  patch: string;
  /** Names the patch in the highlighting cache, e.g. by its commit and path. */
  cacheKey: string;
  diffStyle: DiffStyle;
  loadFile: (oid: string) => Promise<string>;
  /** The last patch given is on show. */
  onShown: () => void;
  /** Whether the whole file is being loaded, to show more of it. */
  onLoadingFiles: (loading: boolean) => void;
  /** Why the whole file couldn't be loaded, e.g. it's too large; `undefined` if it could. */
  onFilesError: (message: string | undefined) => void;
}) {
  const { theme } = useTheme();
  const fileDiff = createMemo(() => parsePatch(props.patch, props.cacheKey));
  let scroller: HTMLDivElement | undefined;
  let content: HTMLDivElement | undefined;
  let instance: VirtualizedFileDiff<undefined, undefined> | undefined;
  /** The diff on show, once there's one. */
  let shown: FileDiffMetadata | undefined;
  /** Loads of the whole file under way, by the diff they're for. */
  const loads = new Map<FileDiffMetadata, number>();
  /** Why the whole file couldn't be loaded, by the diff it's of: it's no use trying again. */
  const failures = new Map<FileDiffMetadata, string>();

  const options = (diffStyle: DiffStyle): FileDiffOptions<undefined, undefined> => ({
    theme: THEMES,
    themeType: theme(),
    diffStyle,
    // The view's own header names the file.
    disableFileHeader: true,
    unsafeCSS: APP_COLORS,
    // Without the buttons to show more, once the whole file couldn't be loaded.
    hunkSeparators: shown && failures.has(shown) ? "simple" : "line-info",
    loadDiffFiles: async (diff) => {
      loads.set(diff, (loads.get(diff) ?? 0) + 1);
      report();
      try {
        return await loadFiles(diff, props.loadFile);
      } catch (error) {
        failures.set(diff, error instanceof Error ? error.message : String(error));
        if (diff === shown) rerender();
        throw error;
      } finally {
        const left = loads.get(diff)! - 1;
        if (left > 0) loads.set(diff, left);
        else loads.delete(diff);
        report();
      }
    },
  });

  /** Says how loading the whole file on show is going. */
  function report() {
    props.onLoadingFiles(shown !== undefined && loads.has(shown));
    props.onFilesError(shown && failures.get(shown));
  }

  /** Renders the diff on show again, with options that may have changed. */
  function rerender() {
    if (!instance || !shown || !content) return;
    instance.setOptions(options(props.diffStyle));
    instance.render({ fileDiff: shown, containerWrapper: content, forceRender: true });
  }

  // Set up once on the page, as the virtualizer measures and observes its scroll container from
  // then on: inside a `Suspense` boundary, it can mount detached (see `useConnected`).
  const connected = useConnected(() => scroller);
  createEffect(() => {
    if (!connected() || !scroller || !content) return;
    untrack(() => setUp(scroller!, content!));
  });

  function setUp(root: HTMLDivElement, wrapper: HTMLDivElement) {
    const virtualizer = new Virtualizer();
    virtualizer.setup(root, wrapper);
    const diffs = new VirtualizedFileDiff(
      options(props.diffStyle),
      virtualizer,
      undefined,
      workerPool(),
    );
    instance = diffs;
    let disposed = false;
    onCleanup(() => {
      disposed = true;
      instance = undefined;
      diffs.cleanUp();
      virtualizer.cleanUp();
    });

    const show = async (diff: FileDiffMetadata) => {
      await highlighted(diff, HIGHLIGHT_WAIT_MS);
      // Passed over for another patch meanwhile.
      if (disposed || fileDiff() !== diff) return;
      // Another file starts at its top.
      if (shown) root.scrollTop = 0;
      shown = diff;
      diffs.setOptions(options(props.diffStyle));
      diffs.render({ fileDiff: diff, containerWrapper: wrapper });
      report();
      props.onShown();
    };
    createEffect(on(fileDiff, (diff) => void show(diff)));
    createEffect(on(() => props.diffStyle, rerender, { defer: true }));
    createEffect(on(theme, (next) => diffs.setThemeType(next), { defer: true }));
  }

  return (
    <div
      ref={(el) => (scroller = el)}
      class="h-full min-h-0 overflow-auto bg-bg [--diffs-font-size:12px] [--diffs-line-height:20px]"
    >
      {/* Never empty: before its first render, a diff without height at the top of the view is
          taken by the virtualizer for one above it, and kept in place by its bottom edge, which
          scrolls the view to the end of the file once its lines are in. */}
      <div ref={(el) => (content = el)} class="[&>*]:min-h-px" />
    </div>
  );
}

/** Resolves once `diff` is highlighted, or `ms` later; never rejects. */
function highlighted(diff: FileDiffMetadata, ms: number): Promise<void> {
  return Promise.race([
    // Shown as plain text if it can't be highlighted.
    workerPool()
      .primeDiffHighlightCache(diff)
      .catch(() => undefined),
    new Promise<void>((resolve) => setTimeout(resolve, ms)),
  ]);
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

/** Both sides of `diff` in full, read by the object names on its `index` line. */
async function loadFiles(
  diff: FileDiffMetadata,
  loadFile: (oid: string) => Promise<string>,
): Promise<FileDiffLoadedFiles> {
  const { prevObjectId, newObjectId } = diff;
  if (!prevObjectId || !newObjectId) throw new Error("The patch doesn't name its files.");
  const [before, after] = await Promise.all([loadFile(prevObjectId), loadFile(newObjectId)]);
  const files = {
    oldFile: { name: diff.prevName ?? diff.name, contents: before, cacheKey: prevObjectId },
    newFile: { name: diff.name, contents: after, cacheKey: newObjectId },
  };
  // Highlighted before they're handed over (see `EXPAND_HIGHLIGHT_WAIT_MS`): a copy, filled in
  // as the viewer will, has the same key in the highlighting cache.
  await highlighted(hydratePartialDiff("clone", diff, files), EXPAND_HIGHLIGHT_WAIT_MS);
  return files;
}
