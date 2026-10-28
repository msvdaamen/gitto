//! What the viewer runs on, shared by every one of it in the app: parsing patches, which is kept
//! for those parsed lately, highlighting them off the main thread, which is kept for a few diffs,
//! and reading whole files into diffs. Ported from `viewer-runtime.ts`, where @pierre/diffs'
//! worker pool highlighted.

use std::collections::HashMap;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::future::Shared;
use gitto_git::ContentVersion;
use gpui_kit::{App, AppContext as _, Global, Task};

use gitto_ui::diff::diff_metadata::{
    ChangeType, FileDiff, HydrateError, LoadedSide, Side, diff_from_files, hydrate,
    parse_patch_files,
};
use gitto_ui::diff::patch_files::{FitError, fit_to_patch, has_matching_ends};

use super::highlight::{DiffHighlights, changed_words, side_syntax};
use crate::data::QueryError;

/// How long a diff waits for its highlighting before it's shown without, until it's
/// highlighted: long enough for most, which then show highlighted at once rather than flicker
/// from plain text, short enough not to hold up one of thousands of lines, which takes a second
/// or more.
pub const HIGHLIGHT_WAIT: Duration = Duration::from_millis(300);

/// How long showing the lines around the changes waits for the whole file to be highlighted: a
/// file of a thousand lines or so would otherwise flicker from plain text.
pub const EXPAND_HIGHLIGHT_WAIT: Duration = Duration::from_millis(1500);

/// How many diffs are kept highlighted, for a quick return to one shown lately, or prepared
/// ahead (see `prepare_patch`): the files a step away. A highlighted line takes memory, so the
/// long ones are dropped once their view closes too (see [`KEPT_LINES`]).
const CACHED_DIFFS: usize = 16;

/// Lines (of both sides) from which a diff's highlighting is dropped once the view that showed it
/// closes, rather than kept among the [`CACHED_DIFFS`]: a whole file read to show more lines, or
/// a long patch. Shorter ones highlight again in well under the time the view waits for that (see
/// [`HIGHLIGHT_WAIT`]), so they're kept for a quick return.
const KEPT_LINES: usize = 1000;

/// One side of a file, read in full: with its version if it's the working tree's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedFile {
    Blob(String),
    WorkingTree {
        contents: String,
        version: ContentVersion,
    },
}

impl LoadedFile {
    pub fn contents(&self) -> &str {
        match self {
            LoadedFile::Blob(contents) | LoadedFile::WorkingTree { contents, .. } => contents,
        }
    }

    pub fn version(&self) -> Option<&ContentVersion> {
        match self {
            LoadedFile::Blob(_) => None,
            LoadedFile::WorkingTree { version, .. } => Some(version),
        }
    }
}

/// Why a patch couldn't be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PatchError {
    #[error("The patch has no file in it.")]
    NoFile,
}

/// Why the file on show couldn't be read whole into its diff, to show the lines around its
/// changes: it couldn't be read, or it isn't the file the patch is of any more (see [`FitError`]),
/// both said to the user as they are; or it was asked of a diff that can't be filled in, which is
/// the viewer's mistake ([`HydrateError`]).
#[derive(Debug, Clone, thiserror::Error)]
pub enum LoadFilesError {
    #[error("The patch doesn't name its files.")]
    Unnamed,
    #[error("{}", .0.message())]
    Read(QueryError),
    #[error(transparent)]
    Fit(#[from] FitError),
    #[error(transparent)]
    Hydrate(#[from] HydrateError),
}

/// Reads one side of the file on show in full, by the object name the patch has for it.
pub type LoadFile = Rc<dyn Fn(Side, &str, &mut App) -> Task<Result<LoadedFile, QueryError>>>;

/// The one file in `patch` (`parsePatch`). A file that changed type, e.g. from a symbolic link, is
/// a deletion and an addition to git, each with the whole of its side: shown as one change, from
/// the one's contents to the other's.
pub fn parse_patch(patch: &str, cache_key: &str) -> Result<FileDiff, PatchError> {
    let mut files = parse_patch_files(patch, Some(cache_key)).into_iter();
    let file = files.next().ok_or(PatchError::NoFile)?;
    if let Some(other) = files.next()
        && file.kind == ChangeType::Deleted
        && other.kind == ChangeType::New
    {
        let old_key = format!("{cache_key}:old");
        let new_key = format!("{cache_key}:new");
        return Ok(diff_from_files(
            &file.name,
            LoadedSide {
                contents: file.deletion_lines.text(),
                cache_key: Some(&old_key),
            },
            &other.name,
            LoadedSide {
                contents: other.addition_lines.text(),
                cache_key: Some(&new_key),
            },
        ));
    }
    Ok(file)
}

/// Patches parsed lately, by their cache key, so one prepared ahead isn't parsed again: at most
/// [`PARSED_PATCHES_KEPT`] of them, with [`PARSED_PATCHES_CHARS`] of patch text between them,
/// besides the latest. Each is kept with its parsed lines, which take as much again: a patch of
/// 10MB would be 20MB kept for as long as the app runs, and sixteen of them hundreds. A long one is
/// parsed again in less than its view waits for its highlighting anyway.
#[derive(Default)]
pub struct ParsedPatches {
    /// Oldest first.
    entries: Vec<ParsedPatch>,
}

const PARSED_PATCHES_KEPT: usize = 16;
const PARSED_PATCHES_CHARS: usize = 4 * 1024 * 1024;

struct ParsedPatch {
    key: String,
    patch: Arc<str>,
    diff: Arc<FileDiff>,
}

impl ParsedPatches {
    /// `patch`, parsed lately with `key`, if it was.
    pub fn get(&self, patch: &str, key: &str) -> Option<Arc<FileDiff>> {
        self.entries
            .iter()
            .find(|entry| entry.key == key && *entry.patch == *patch)
            .map(|entry| Arc::clone(&entry.diff))
    }

    /// Keeps `diff`, parsed from `patch` with `key`, as the latest: the older ones go once
    /// there are too many, or too much text among them. The latest always stays, so the viewer
    /// gets the same diff for the same patch while it's on show.
    pub fn insert(&mut self, key: &str, patch: Arc<str>, diff: Arc<FileDiff>) {
        self.entries.retain(|entry| entry.key != key);
        self.entries.push(ParsedPatch {
            key: key.to_owned(),
            patch,
            diff,
        });
        let mut chars = 0;
        let mut keep = vec![true; self.entries.len()];
        for (age, (ix, entry)) in self.entries.iter().enumerate().rev().enumerate() {
            chars += entry.patch.len();
            if age >= PARSED_PATCHES_KEPT || (age > 0 && chars > PARSED_PATCHES_CHARS) {
                keep[ix] = false;
            }
        }
        let mut keep = keep.into_iter();
        self.entries.retain(|_| keep.next().unwrap_or(true));
    }

    /// [`parse_patch`], of a patch that may have been parsed lately (`parsed`).
    pub fn parsed(&mut self, patch: &Arc<str>, key: &str) -> Result<Arc<FileDiff>, PatchError> {
        if let Some(diff) = self.get(patch, key) {
            return Ok(diff);
        }
        let diff = Arc::new(parse_patch(patch, key)?);
        self.insert(key, Arc::clone(patch), Arc::clone(&diff));
        Ok(diff)
    }
}

/// A diff's highlighting, under way or done: awaited by whoever needs it, read once it's done.
pub type SharedHighlights = Shared<Task<Arc<DiffHighlights>>>;

/// The patches parsed lately, and the diffs highlighted lately, for every viewer in the app.
#[derive(Default)]
pub struct ViewerRuntime {
    parsed: ParsedPatches,
    /// Oldest first.
    highlights: Vec<(String, SharedHighlights)>,
}

impl Global for ViewerRuntime {}

impl ViewerRuntime {
    fn global(cx: &mut App) -> &mut ViewerRuntime {
        cx.default_global::<ViewerRuntime>()
    }

    /// `patch`, parsed with `key` off the main thread, or as it was lately: a patch can be
    /// megabytes.
    pub fn parse(
        patch: Arc<str>,
        key: String,
        cx: &mut App,
    ) -> Task<Result<Arc<FileDiff>, PatchError>> {
        if let Some(diff) = Self::global(cx).parsed.get(&patch, &key) {
            return Task::ready(Ok(diff));
        }
        cx.spawn(async move |cx| {
            let (patch, key, parsed) = cx
                .background_spawn(async move {
                    let parsed = parse_patch(&patch, &key).map(Arc::new);
                    (patch, key, parsed)
                })
                .await;
            let diff = parsed?;
            cx.update(|cx| {
                Self::global(cx)
                    .parsed
                    .insert(&key, patch, Arc::clone(&diff))
            });
            Ok(diff)
        })
    }

    /// `diff`'s highlighting: started now, off the main thread, unless it's under way or done
    /// already. Kept for the [`CACHED_DIFFS`] last asked for, if the diff is named.
    pub fn highlight(diff: &Arc<FileDiff>, cx: &mut App) -> SharedHighlights {
        let runtime = Self::global(cx);
        if let Some(key) = &diff.cache_key
            && let Some(ix) = runtime.highlights.iter().position(|(kept, _)| kept == key)
        {
            let entry = runtime.highlights.remove(ix);
            let highlights = entry.1.clone();
            runtime.highlights.push(entry);
            return highlights;
        }
        // The sides are parsed at once, and the changed words worked out meanwhile, each a task
        // of the executor's: a modified file's two sides are two whole files, which parsing one
        // after the other would take twice as long to color. Dropped, the tasks not yet started
        // never are.
        let syntax = |side, cx: &mut App| {
            let diff = Arc::clone(diff);
            cx.background_spawn(async move { side_syntax(&diff, side) })
        };
        let deletions = syntax(Side::Deletions, cx);
        let additions = syntax(Side::Additions, cx);
        let changed = cx.background_spawn({
            let diff = Arc::clone(diff);
            async move { changed_words(&diff) }
        });
        let highlights = cx
            .background_spawn(async move {
                let (deletions, additions, changed) = futures::join!(deletions, additions, changed);
                Arc::new(DiffHighlights::of_parts(deletions, additions, changed))
            })
            .shared();
        if let Some(key) = &diff.cache_key {
            let runtime = Self::global(cx);
            runtime.highlights.push((key.clone(), highlights.clone()));
            if runtime.highlights.len() > CACHED_DIFFS {
                // Dropped: whoever still awaits it keeps it until it's done.
                drop(runtime.highlights.remove(0));
            }
        }
        highlights
    }

    /// Drops the highlighting kept as `key`.
    pub fn evict(key: &str, cx: &mut App) {
        Self::global(cx).highlights.retain(|(kept, _)| kept != key);
    }

    /// Whether the highlighting kept as `key` is there, done or under way: for tests.
    #[cfg(test)]
    pub fn has_highlights(key: &str, cx: &mut App) -> bool {
        Self::global(cx)
            .highlights
            .iter()
            .any(|(kept, _)| kept == key)
    }
}

/// Resolves once `diff` is highlighted, or `wait` later; never fails.
pub fn highlighted(diff: &Arc<FileDiff>, wait: Duration, cx: &mut App) -> Task<()> {
    let highlights = ViewerRuntime::highlight(diff, cx);
    let timer = cx.background_executor().timer(wait);
    cx.background_spawn(async move {
        futures::future::select(pin!(highlights), pin!(timer)).await;
    })
}

/// Parses and highlights a patch ahead of showing it, with the same `cache_key`, so it shows at
/// once (`preparePatch`). Fails if it can't be parsed.
pub fn prepare_patch(
    patch: Arc<str>,
    cache_key: String,
    cx: &mut App,
) -> Task<Result<(), PatchError>> {
    let parsing = ViewerRuntime::parse(patch, cache_key, cx);
    cx.spawn(async move |cx| {
        let diff = parsing.await?;
        let highlights = cx.update(|cx| ViewerRuntime::highlight(&diff, cx));
        highlights.await;
        Ok(())
    })
}

/// The diffs one view had highlighted, to drop the long ones from the cache once the view closes
/// (see [`CACHED_DIFFS`], [`KEPT_LINES`]): the whole files read to show more lines, and long
/// patches, which it would otherwise keep for as long as the app runs.
#[derive(Default)]
pub struct ViewHighlights {
    /// The lines of each diff, by its key.
    diffs: HashMap<String, usize>,
}

impl ViewHighlights {
    /// Remembers `diff` as one the view had highlighted, if it's named in the cache.
    pub fn remember(&mut self, diff: &FileDiff) {
        if let Some(key) = &diff.cache_key {
            self.diffs.insert(
                key.clone(),
                diff.addition_lines.len() + diff.deletion_lines.len(),
            );
        }
    }

    /// [`highlighted`], remembering the diff.
    pub fn highlight(&mut self, diff: &Arc<FileDiff>, wait: Duration, cx: &mut App) -> Task<()> {
        self.remember(diff);
        highlighted(diff, wait, cx)
    }

    /// Drops `diff` from the cache at once, replaced by a newer one while the view's open, and
    /// lets go of it: a file can be saved many times meanwhile (`forget`).
    pub fn forget(&mut self, diff: &FileDiff, cx: &mut App) {
        let Some(key) = &diff.cache_key else {
            return;
        };
        ViewerRuntime::evict(key, cx);
        if let Some(partial) = key.strip_suffix(":hydrated") {
            ViewerRuntime::evict(partial, cx);
        }
        self.diffs.remove(key);
    }

    /// Drops the long diffs from the cache, as the view closes; the shorter ones stay.
    pub fn drop_long(&mut self, cx: &mut App) {
        for (key, lines) in self.diffs.drain() {
            if lines >= KEPT_LINES {
                ViewerRuntime::evict(&key, cx);
            }
        }
    }
}

/// Both sides of a file read in full, as a diff of its patch filled in with them.
#[derive(Debug, Clone)]
pub struct LoadedFiles {
    pub hydrated: Arc<FileDiff>,
    /// The working tree's version of the new side, if it's the working tree's.
    pub version: Option<ContentVersion>,
}

/// Both sides of `diff` in full, read by the object names on its `index` line, and a copy of
/// `diff` filled in with them (`loadFiles`). Fails if they aren't the files the patch is of, as a
/// file in the working tree can have changed since; with why.
pub fn load_files(
    diff: Arc<FileDiff>,
    load_file: &LoadFile,
    cx: &mut App,
) -> Task<Result<LoadedFiles, LoadFilesError>> {
    let (Some(prev), Some(new)) = (diff.prev_object_id.clone(), diff.new_object_id.clone()) else {
        return Task::ready(Err(LoadFilesError::Unnamed));
    };
    let old_file = load_file(Side::Deletions, &prev, cx);
    let new_file = load_file(Side::Additions, &new, cx);
    cx.spawn(async move |cx| {
        let old = old_file.await.map_err(LoadFilesError::Read)?;
        let read = new_file.await.map_err(LoadFilesError::Read)?;
        let version = read.version().cloned();
        let hydrated = cx
            .background_spawn(async move {
                let before = fit_to_patch(&diff, Side::Deletions, old.contents())?;
                let after = fit_to_patch(&diff, Side::Additions, read.contents())?;
                // Named in the highlighting cache by the patch's object names: the working tree's
                // side is by what git would store it as, so another version of the file has
                // another name too.
                let hydrated = hydrate(
                    &diff,
                    Some(LoadedSide {
                        contents: &before,
                        cache_key: Some(&prev),
                    }),
                    Some(LoadedSide {
                        contents: &after,
                        cache_key: Some(&new),
                    }),
                )?;
                if !has_matching_ends(&hydrated) {
                    return Err(FitError::Changed.into());
                }
                Ok::<_, LoadFilesError>(Arc::new(hydrated))
            })
            .await?;
        // Highlighted before it's handed over (see `EXPAND_HIGHLIGHT_WAIT`).
        cx.update(|cx| highlighted(&hydrated, EXPAND_HIGHLIGHT_WAIT, cx))
            .await;
        Ok(LoadedFiles { hydrated, version })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A patch adding `lines` lines, each `width` characters wide, as the file `name`.
    fn patch_of(name: &str, lines: usize, width: usize) -> Arc<str> {
        let line = "x".repeat(width);
        let mut patch = vec![
            format!("diff --git a/{name} b/{name}"),
            "new file mode 100644".to_owned(),
            "index 0000000000000000000000000000000000000000..4cb29ea38f70d7c61b2a3a25b02e3bdf44905402".to_owned(),
            "--- /dev/null".to_owned(),
            format!("+++ b/{name}"),
            format!("@@ -0,0 +1,{lines} @@"),
        ];
        patch.extend((0..lines).map(|_| format!("+{line}")));
        patch.push(String::new());
        patch.join("\n").into()
    }

    // parsePatch

    #[test]
    fn shows_a_file_that_changed_type_as_one_change() {
        // A symbolic link replaced by a file, as `git diff-tree -p` has it.
        let patch = [
            "diff --git a/link b/link",
            "deleted file mode 120000",
            "index 4cbb553f3f4ac2ee7b01ff6c951d6bf583c39c15..0000000000000000000000000000000000000000",
            "--- a/link",
            "+++ /dev/null",
            "@@ -1 +0,0 @@",
            "-target.txt",
            "\\ No newline at end of file",
            "diff --git a/link b/link",
            "new file mode 100644",
            "index 0000000000000000000000000000000000000000..4cb29ea38f70d7c61b2a3a25b02e3bdf44905402",
            "--- /dev/null",
            "+++ b/link",
            "@@ -0,0 +1,3 @@",
            "+one",
            "+two",
            "+three",
            "",
        ]
        .join("\n");

        let diff = parse_patch(&patch, "a1:link").unwrap();
        assert_eq!(diff.kind, ChangeType::Change);
        assert_eq!(
            diff.deletion_lines.iter().collect::<Vec<_>>(),
            ["target.txt"]
        );
        assert_eq!(
            diff.addition_lines.iter().collect::<Vec<_>>(),
            ["one\n", "two\n", "three\n"]
        );
    }

    #[test]
    fn says_a_patch_without_a_file_has_none() {
        assert_eq!(parse_patch("", "empty").unwrap_err(), PatchError::NoFile);
    }

    // parsed

    #[gpui_kit::test]
    async fn highlights_both_sides_and_the_changed_words_as_tasks_of_their_own(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let diff = Arc::new(
            parse_patch_files(
                "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-let a = 1;\n+let total = 2;\n",
                Some("test"),
            )
            .remove(0),
        );
        let highlights = cx.update(|cx| ViewerRuntime::highlight(&diff, cx)).await;
        // As worked out one part after the other.
        assert_eq!(*highlights, DiffHighlights::compute(&diff));
        assert!(!highlights.syntax(Side::Deletions, 0).is_empty());
        assert!(!highlights.syntax(Side::Additions, 0).is_empty());
        assert!(!highlights.changed(Side::Additions, 0).is_empty());
    }

    #[test]
    fn keeps_a_patch_parsed_lately_so_it_isnt_parsed_again() {
        let mut parsed = ParsedPatches::default();
        let patch = patch_of("kept.txt", 3, 10);
        let first = parsed.parsed(&patch, "kept").unwrap();
        assert!(Arc::ptr_eq(&first, &parsed.parsed(&patch, "kept").unwrap()));
    }

    #[test]
    fn parses_a_patch_again_once_it_changed() {
        let mut parsed = ParsedPatches::default();
        let first = parsed
            .parsed(&patch_of("changed.txt", 3, 10), "changed")
            .unwrap();
        let second = parsed
            .parsed(&patch_of("changed.txt", 4, 10), "changed")
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn lets_go_of_a_long_patch_once_a_newer_one_comes_but_not_of_the_latest() {
        let mut parsed = ParsedPatches::default();
        // Longer than all the patches kept together may be.
        let long = patch_of("long.txt", 50_000, 100);
        let short = patch_of("short.txt", 3, 10);
        let kept = parsed.parsed(&long, "long").unwrap();
        assert!(Arc::ptr_eq(&kept, &parsed.parsed(&long, "long").unwrap()));

        let short_diff = parsed.parsed(&short, "short").unwrap();
        assert!(Arc::ptr_eq(
            &short_diff,
            &parsed.parsed(&short, "short").unwrap()
        ));
        assert!(!Arc::ptr_eq(&kept, &parsed.parsed(&long, "long").unwrap()));
    }

    #[test]
    fn keeps_the_latest_sixteen_short_patches() {
        let mut parsed = ParsedPatches::default();
        let patches: Vec<_> = (0..17)
            .map(|i| patch_of(&format!("file-{i}.txt"), 3, 10))
            .collect();
        let diffs: Vec<_> = patches
            .iter()
            .enumerate()
            .map(|(i, patch)| parsed.parsed(patch, &format!("file-{i}")).unwrap())
            .collect();
        assert!(Arc::ptr_eq(
            &parsed.parsed(&patches[1], "file-1").unwrap(),
            &diffs[1]
        ));
        assert!(!Arc::ptr_eq(
            &parsed.parsed(&patches[0], "file-0").unwrap(),
            &diffs[0]
        ));
    }
}
