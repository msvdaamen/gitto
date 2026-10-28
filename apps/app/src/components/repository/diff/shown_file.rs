//! What the diff view shows of one file (see `FileDiffView`): the file as its list has it now, the
//! patch of its changes, and what's on show while the next one loads. Kept in one place, which
//! staging the file and resolving its conflicts read from, rather than each deriving its own.
//! Ported from `shown-file.ts`.

use std::sync::Arc;

use gitto_git::FileStatus;
use gitto_git::features::diff::schema::ChangedFile;
use gitto_git::features::staging::patch_version::patch_version;
use gitto_git::input::RepositoryId;
use gitto_ui_logic::git::changes::{
    LineCounts, counts_of, has_patch, is_known_binary, is_nested_repository, total_lines,
};
use gitto_ui_logic::git::patch::{PatchSummary, is_link, summarize_patch};
use gpui_kit::{AppContext as _, Context, Entity, Subscription};

use gitto_app_data::data::file_diff::{DiffSourceFiles, FilePatch};
use gitto_app_data::data::keyed::Hold;
use gitto_app_data::data::{
    DataError, Loaded, Opaque, Part, Repositories, Repository, observe_parts,
};
use gitto_app_kit::delayed::{DELAY, Delayed};
use gitto_ui_logic::git::diff_source::{DiffSource, diff_file_key, is_same_source, is_uncommitted};

/// Changed lines above which a file's changes are only shown when asked: reading and parsing them
/// takes a noticeable moment, and such a change is rarely read line by line (a lockfile, say).
pub const LARGE_DIFF_LINES: u64 = 20_000;

/// Names a file's patch in the highlighting cache. A commit's or a stash's never changes, so its
/// file's name will do; an uncommitted one's is named by what's in it too (see `patch_version`).
pub fn patch_cache_key(source: &DiffSource, data: &FilePatch) -> String {
    if is_uncommitted(source) {
        format!("{}:{}", data.key, patch_version(&data.patch))
    } else {
        data.key.clone()
    }
}

/// The patch on show, and the file it's of.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownPatch {
    pub key: String,
    pub file: ChangedFile,
    pub patch: Arc<str>,
    pub counts: Option<LineCounts>,
}

/// The file in the header: the one asked for, or the last one, while its patch is on its way.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderFile {
    pub key: String,
    pub file: ChangedFile,
    pub counts: Option<LineCounts>,
}

/// The file `file` of `source`, as the view shows it (`createShownFile`): as its list has it now,
/// with the patch of its changes, and what's on show meanwhile. Going to another file, the last
/// one stays on show, header and all, until the new one's patch is loaded and highlighted.
///
/// While the file's edited ([`set_editing`](Self::set_editing)), it and its patch stay as they
/// were when that started: saving the edits changes it, and could take it from its list, or leave
/// it without changes, which isn't shown in the editor.
///
/// Kept by the view, which [`sync`](Self::sync)s it with what's loaded before it reads it.
pub struct ShownFile {
    repository_id: RepositoryId,
    repository: Entity<Repository>,
    source: DiffSource,
    file: ChangedFile,
    /// The files whose changes the source has, to step through.
    lists: DiffSourceFiles,
    /// Where the file was in its list, after it's gone from it: the files around it stay a step
    /// away. Only this file's place: another one opened that isn't in its list has none.
    last_place: (String, Option<usize>),
    /// The large file the user asked to see anyway.
    shown_large: Option<String>,
    /// What the patch held is of, changing only when that does: an uncommitted file's list is
    /// replaced whenever anything in the working tree changes.
    patch_file: ChangedFile,
    patch_enabled: bool,
    /// Holds the patch while it's loaded (see `sync`).
    patch_hold: Option<Hold>,
    /// The patch loaded last, as last synced: this file's, or the last one's while this one's
    /// loads.
    patch: Option<FilePatch>,
    /// Why this file's patch couldn't be loaded, as last synced.
    patch_error: Option<DataError>,
    /// The patch on show: going to another file, the last one stays on show until the new one's
    /// is loaded and highlighted; they're swapped at once, without flicker.
    shown_patch: Option<ShownPatch>,
    /// Whether the whole file is being loaded, to show more of it, and why it couldn't be.
    loading_files: bool,
    files_error: Option<String>,
    /// Whether changes are being loaded, for long enough to say so.
    busy: Entity<Delayed>,
    /// While the file's edited: the file, and its patch, as they were when that started.
    editing: Option<(ChangedFile, Option<FilePatch>)>,
    _changes: Subscription,
}

/// What a file's view shows of the repository: the lists, and the patch.
const PARTS: &[Part] = &[Part::Status, Part::Commits, Part::Patches];

impl ShownFile {
    pub fn new<V: 'static>(
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        cx: &mut Context<V>,
    ) -> Self {
        let repository = Repositories::open(&repository_id, cx);
        let lists = DiffSourceFiles::new(&repository, source.clone(), cx);
        let busy = cx.new(|cx| Delayed::new(false, DELAY, cx));
        let changes = observe_parts(&repository, PARTS, cx);
        let mut shown = ShownFile {
            repository_id,
            repository,
            source,
            patch_file: file.clone(),
            file,
            lists,
            last_place: (String::new(), None),
            shown_large: None,
            patch_enabled: false,
            patch_hold: None,
            patch: None,
            patch_error: None,
            shown_patch: None,
            loading_files: false,
            files_error: None,
            busy,
            editing: None,
            _changes: changes,
        };
        shown.sync(cx);
        shown
    }

    /// Shows `file` of `source` instead.
    pub fn set_file<V: 'static>(
        &mut self,
        repository_id: RepositoryId,
        source: DiffSource,
        file: ChangedFile,
        cx: &mut Context<V>,
    ) {
        if repository_id != self.repository_id {
            self.repository = Repositories::open(&repository_id, cx);
            self.lists = DiffSourceFiles::new(&self.repository, source.clone(), cx);
            self._changes = observe_parts(&self.repository, PARTS, cx);
            self.repository_id = repository_id;
            // Of another repository: its patch is held anew.
            self.patch_hold = None;
            self.patch_enabled = false;
        } else if !is_same_source(&self.source, &source) {
            self.lists.set_source(source.clone(), cx);
        }
        self.source = source;
        self.file = file;
        self.sync(cx);
    }

    /// Brings what follows from the lists and the file up to date: its place in its list, and
    /// what the patch held is of, and what of it's loaded.
    pub fn sync<V: 'static>(&mut self, cx: &mut Context<V>) {
        self.lists.sync(cx);
        let key = self.file_key();
        self.last_place = match self.found() {
            Some(ix) => (key, Some(ix)),
            None if self.last_place.0 == key => self.last_place.clone(),
            None => (key, None),
        };
        // Not loaded once the file's gone from its list, nor for a binary one, or a large one
        // until it's asked for.
        let file = self.file();
        // Not reloaded while it's edited, as each save would: the view keeps the patch editing
        // started from, and gets the last one once editing stops.
        let enabled = self.editing.is_none() && !self.gone() && self.fetches();
        let same = file.path == self.patch_file.path
            && file.orig_path == self.patch_file.orig_path
            && file.status == self.patch_file.status;
        if !same || enabled != self.patch_enabled || (enabled && self.patch_hold.is_none()) {
            let source = self.source.clone();
            self.patch_hold = enabled.then(|| {
                self.repository.update(cx, |repository, cx| {
                    repository.hold_patch(&source, &file, cx)
                })
            });
            self.patch_file = file;
            self.patch_enabled = enabled;
        }
        let loaded = self
            .repository
            .read(cx)
            .patch(&self.source, &self.patch_file.path);
        if let Some(patch) = loaded.and_then(Loaded::data) {
            self.patch = Some(patch.clone());
        }
        self.patch_error = loaded
            .and_then(Loaded::error)
            .filter(|_| self.patch_enabled)
            .cloned();
        let busy = self.shows_other() || self.loading_files;
        self.busy.update(cx, |delayed, cx| delayed.set(busy, cx));
    }

    pub fn repository_id(&self) -> &RepositoryId {
        &self.repository_id
    }

    /// The repository the file is in.
    pub fn repository(&self) -> &Entity<Repository> {
        &self.repository
    }

    pub fn source(&self) -> &DiffSource {
        &self.source
    }

    /// The files whose changes the source has, to step through.
    pub fn lists(&self) -> &DiffSourceFiles {
        &self.lists
    }

    /// Names the file in its source (see `DiffSource::file_key`).
    pub fn file_key(&self) -> String {
        diff_file_key(&self.source, &self.file.path)
    }

    /// Where the file is in its list, if it is.
    fn found(&self) -> Option<usize> {
        self.lists
            .files()
            .iter()
            .position(|file| file.path == self.file.path)
    }

    /// Whether the file is still in its list.
    pub fn listed(&self) -> bool {
        self.found().is_some()
    }

    fn files(&self) -> Opaque<Vec<ChangedFile>> {
        self.lists.files()
    }

    /// The file a step up in the list, if there's one.
    pub fn previous(&self) -> Option<ChangedFile> {
        let place = self.last_place.1?;
        place
            .checked_sub(1)
            .and_then(|ix| self.files().get(ix).cloned())
    }

    /// The file a step down in the list, if there's one: the one in its place, once it's gone.
    pub fn next(&self) -> Option<ChangedFile> {
        let place = self.last_place.1?;
        let ix = if self.found().is_none() {
            place
        } else {
            place + 1
        };
        self.files().get(ix).cloned()
    }

    /// The file to open once this one has no changes left on its side: the next in its list, or
    /// the one before the last, as one would stage them one after the other.
    pub fn target(&self) -> Option<ChangedFile> {
        self.next().or_else(|| self.previous())
    }

    /// The file as its list has it now: uncommitted changes change on disk, and are listed anew.
    /// As it was opened if it's gone from the list.
    pub fn live_file(&self) -> ChangedFile {
        if is_uncommitted(&self.source)
            && let Some(ix) = self.found()
            && let Some(file) = self.files().get(ix)
        {
            return file.clone();
        }
        self.file.clone()
    }

    /// The file on show: as it was when editing started, while it's edited.
    pub fn file(&self) -> ChangedFile {
        match &self.editing {
            Some((file, _)) => file.clone(),
            None => self.live_file(),
        }
    }

    /// Whether the file has gone from its list: it's been staged in full, say. Not while it's
    /// edited.
    pub fn gone(&self) -> bool {
        self.editing.is_none()
            && is_uncommitted(&self.source)
            && self.lists.loaded()
            && self.found().is_none()
    }

    /// Whether the file's being edited by hand (see `file_editing`).
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// The file's being edited by hand, or no longer: it, and its patch, stay as they are until
    /// editing stops.
    pub fn set_editing<V: 'static>(&mut self, editing: bool, cx: &mut Context<V>) {
        if editing == self.editing.is_some() {
            return;
        }
        self.editing = editing.then(|| (self.live_file(), self.patch.clone()));
        self.sync(cx);
    }

    /// Whether its patch is loaded: one that has one, not too large or asked for anyway.
    fn fetches(&self) -> bool {
        let file = self.file();
        if !has_patch(&file, self.lists.uncounted()) {
            return false;
        }
        match counts_of(&file) {
            None => true,
            Some(counts) => {
                total_lines(counts) <= LARGE_DIFF_LINES
                    || self.shown_large.as_deref() == Some(self.file_key().as_str())
            }
        }
    }

    /// The patch loaded last: this file's, or the last one's while this one's loads; the one
    /// editing started from, while the file's edited.
    pub fn patch(&self) -> Option<FilePatch> {
        match &self.editing {
            Some((_, patch)) => patch.clone(),
            None => self.patch.clone(),
        }
    }

    /// Why the patch couldn't be loaded; nothing while the file's edited, as the patch on show is
    /// the one editing started from.
    pub fn patch_error(&self) -> Option<DataError> {
        if self.editing.is_some() {
            return None;
        }
        self.patch_error.clone()
    }

    /// The patch of this file, if it's loaded.
    pub fn current(&self) -> Option<FilePatch> {
        self.patch().filter(|data| data.key == self.file_key())
    }

    /// What the patch says of the changes: an uncommitted file's line counts can be older than
    /// its patch, and some files have none at all.
    pub fn summary_of(&self, data: &FilePatch) -> Option<PatchSummary> {
        (is_uncommitted(&self.source) || data.file.additions.is_none())
            .then(|| summarize_patch(&data.patch))
    }

    fn summary(&self) -> Option<PatchSummary> {
        self.current().and_then(|data| self.summary_of(&data))
    }

    /// Whether the file has no changes left on its side, as its patch says.
    pub fn empty(&self) -> bool {
        self.current().is_some_and(|data| data.patch.is_empty())
    }

    pub fn binary(&self) -> bool {
        is_known_binary(&self.file(), self.lists.uncounted())
            || self.summary().is_some_and(|summary| summary.binary)
    }

    /// How many lines changed; `None` until the patch says, for a file without counts.
    pub fn counts(&self) -> Option<LineCounts> {
        match self.summary() {
            Some(summary) if !summary.binary => Some(LineCounts {
                additions: summary.additions,
                deletions: summary.deletions,
            }),
            _ => counts_of(&self.file()),
        }
    }

    pub fn lines(&self) -> Option<u64> {
        self.counts().map(total_lines)
    }

    /// Whether the change is large, and the user hasn't asked to see it anyway.
    pub fn large(&self) -> bool {
        self.lines().unwrap_or(0) > LARGE_DIFF_LINES
            && self.shown_large.as_deref() != Some(self.file_key().as_str())
    }

    /// Shows the large change anyway.
    pub fn show_large<V: 'static>(&mut self, cx: &mut Context<V>) {
        self.shown_large = Some(self.file_key());
        self.sync(cx);
    }

    pub fn conflicted(&self) -> bool {
        self.file().status == FileStatus::Conflicted
    }

    pub fn nested(&self) -> bool {
        is_nested_repository(&self.file())
    }

    /// Whether the file's changes are shown as a patch, rather than said to be binary, say.
    pub fn shows_patch(&self) -> bool {
        !self.gone()
            && !self.empty()
            && !self.conflicted()
            && !self.nested()
            && !self.binary()
            && self.lines() != Some(0)
            && !self.large()
    }

    /// The patch on show, if one is: the last file's, while this one's loads.
    pub fn shown_patch(&self) -> Option<&ShownPatch> {
        self.shown_patch.as_ref()
    }

    /// The file on show: the one asked for, or the last one, while its patch is on its way.
    pub fn shown(&self) -> HeaderFile {
        if self.shows_patch()
            && let Some(shown) = &self.shown_patch
        {
            return HeaderFile {
                key: shown.key.clone(),
                file: shown.file.clone(),
                counts: shown.counts,
            };
        }
        HeaderFile {
            key: self.file_key(),
            file: self.file(),
            // None to count once it has no changes left on its side, nor for a conflict: git
            // counts them against one side.
            counts: if self.gone() || self.empty() || self.conflicted() {
                None
            } else {
                self.counts()
            },
        }
    }

    /// Whether the file in the header is another one, still on show while this one's changes
    /// load.
    pub fn shows_other(&self) -> bool {
        self.shown().key != self.file_key()
    }

    /// Whether changes are being loaded, for long enough to say so.
    pub fn busy(&self) -> &Entity<Delayed> {
        &self.busy
    }

    /// Why the whole file couldn't be loaded, to show more of it.
    pub fn files_error(&self) -> Option<&str> {
        self.files_error.as_deref()
    }

    pub fn set_loading_files<V: 'static>(&mut self, loading: bool, cx: &mut Context<V>) {
        self.loading_files = loading;
        self.sync(cx);
    }

    pub fn set_files_error(&mut self, error: Option<String>) {
        self.files_error = error;
    }

    /// Whether the patch on show can be edited: an unstaged one's new side, the file on disk, if
    /// it's text that's there, and not a link (`editablePatch`).
    pub fn editable_patch(&self) -> bool {
        self.source == DiffSource::Unstaged
            && self.shows_patch()
            && !self.shows_other()
            && self.file().status != FileStatus::Deleted
            && !self.current().is_some_and(|data| is_link(&data.patch))
    }

    /// The patch `data` is on show.
    pub fn patch_shown<V: 'static>(&mut self, data: &FilePatch, cx: &mut Context<V>) {
        let counts = self
            .summary_of(data)
            .map(|summary| LineCounts {
                additions: summary.additions,
                deletions: summary.deletions,
            })
            .or_else(|| counts_of(&data.file));
        self.shown_patch = Some(ShownPatch {
            key: data.key.clone(),
            file: data.file.clone(),
            patch: Arc::clone(&data.patch),
            counts,
        });
        self.sync(cx);
    }

    /// No patch is on show: its viewer left the page.
    pub fn patch_left<V: 'static>(&mut self, cx: &mut Context<V>) {
        self.shown_patch = None;
        self.loading_files = false;
        self.files_error = None;
        self.sync(cx);
    }
}
