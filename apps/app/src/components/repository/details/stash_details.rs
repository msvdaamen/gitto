//! A stash's details. Ported from `components/repository/details/stash-details.tsx`.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::rc::Rc;

use gitto_git::features::stash::schema::Stash;
use gitto_git::input::RepositoryId;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, EventEmitter, FontWeight, Subscription, Window, div, rems};

use super::changed_file_list::{ChangedFileList, HoverIntent};
use super::commit_details::{CommitDetailsEvent, ShownFile, openable_files};
use super::details_sections::{DetailsError, DetailsList, FileTotals, RenderTop};
use gitto_app_data::data::diff::{FilesOf, FilesOnShow};
use gitto_app_data::data::{Opaque, Part, Shown};
use gitto_app_data::state::clock::{Clock, relative_time};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Icon, IconName, LoadingState, Tone};
use gitto_ui_logic::git::changes::LineCounts;
use gitto_ui_logic::git::diff_source::DiffSource;

/// What the stash details show of the repository.
const PARTS: &[Part] = &[Part::Stashes, Part::Commits];

/// A stash selected in the history: what git named it, when it was made, and the files it
/// changed, scrolled with the summary.
pub struct StashDetails {
    repository: Shown,
    sha: String,
    files: FilesOnShow,
    shown_file: ShownFile,
    list: DetailsList,
    hover: Entity<HoverIntent>,
    _clock: Subscription,
}

impl EventEmitter<CommitDetailsEvent> for StashDetails {}

impl StashDetails {
    pub fn new(repository_id: &RepositoryId, sha: &str, cx: &mut Context<Self>) -> Self {
        let repository = Shown::new(repository_id, PARTS, cx);
        let files = FilesOnShow::new(FilesOf::Stash, sha, repository.entity(), cx);
        let clock = Clock::global(cx);
        StashDetails {
            repository,
            sha: sha.to_owned(),
            files,
            shown_file: None,
            list: DetailsList::new(),
            hover: cx.new(|_| HoverIntent::default()),
            _clock: cx.observe(&clock, |_, _, cx| cx.notify()),
        }
    }

    /// Shows the stash `sha` instead; the one on show stays until its files are loaded.
    pub fn set_sha(&mut self, sha: &str, cx: &mut Context<Self>) {
        if self.sha == sha {
            return;
        }
        self.sha = sha.to_owned();
        self.files.set_sha(sha, self.repository.entity(), cx);
        cx.notify();
    }

    pub fn set_shown_file(&mut self, shown: ShownFile, cx: &mut Context<Self>) {
        if self.shown_file != shown {
            self.shown_file = shown;
            cx.notify();
        }
    }
}

impl Render for StashDetails {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let repository = self.repository.read(cx);
        let files = self.files.data(repository);
        let shown_sha = files.as_ref().map(|files| files.shown_sha.clone());
        // The stash whose files are on show, so the summary never names another one while its
        // files load. Gone once it's popped; the history then selects another row.
        let shown = repository.stashes().data().and_then(|stashes| {
            stashes
                .iter()
                .find(|stash| Some(&stash.sha) == shown_sha.as_ref())
                .cloned()
        });
        let error = repository
            .stashes()
            .error()
            .cloned()
            .or_else(|| self.files.error(repository));
        let loading = files.is_none() && error.is_none();
        let (count, totals) = files.as_ref().map_or((0, LineCounts::default()), |files| {
            (files.files.len(), files.totals)
        });
        let top: RenderTop = {
            let error = error.clone();
            let shown = shown.clone();
            Rc::new(move |_, _| {
                v_flex()
                    .w_full()
                    .child(DetailsError::new(
                        "stash-error",
                        "Couldn't load the stash",
                        error.clone(),
                    ))
                    .when(loading, |this| {
                        this.child(LoadingState::new("loading-details", "Loading details…"))
                    })
                    .when_some(shown.clone(), |this, stash| {
                        this.child(StashSummary {
                            stash,
                            count,
                            totals,
                        })
                    })
                    .into_any_element()
            })
        };
        let mut hasher = DefaultHasher::new();
        (
            shown.as_ref().map(|stash| stash.sha.clone()),
            error.as_ref().map(|error| error.to_string()),
            loading,
        )
            .hash(&mut hasher);

        let source = DiffSource::Stash {
            sha: self.sha.clone(),
        };
        let mut list = ChangedFileList::new(
            files
                .map(|files| files.files)
                .unwrap_or_else(|| Opaque::new(Vec::new())),
        );
        // Only once the files on show are this stash's, so another's aren't opened as its.
        if shown_sha.as_deref() == Some(self.sha.as_str()) {
            list = openable_files(list, source, &self.shown_file, &self.hover, cx);
        }
        self.list
            .render("stash-files", top, hasher.finish(), list, cx)
    }
}

/// The stash's name, when it was made, its SHA, and how much it changed.
#[derive(IntoElement)]
struct StashSummary {
    stash: Stash,
    count: usize,
    totals: LineCounts,
}

impl RenderOnce for StashSummary {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (background, foreground) = Tone::Amber.colors(&palette);
        let short_sha: String = self.stash.sha.chars().take(7).collect();
        v_flex()
            .items_start()
            .p_4()
            .border_b_1()
            .border_color(palette.border)
            // A badge, with the stash's icon in it.
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .px_1p5()
                    .rounded_full()
                    .border_1()
                    .border_color(Tone::Amber.border(&palette))
                    .bg(background)
                    .text_color(foreground)
                    .text_xs()
                    .line_height(rems(1.1))
                    .font_weight(FontWeight::BOLD)
                    .child(Icon::new(IconName::Archive).size_3())
                    .child("Stash"),
            )
            .child(
                div()
                    .mt_3()
                    .mb_1p5()
                    .text_sm()
                    .line_height(rems(1.25))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette.text)
                    .child(self.stash.message.clone()),
            )
            .child(div().text_xs().text_color(palette.muted).child(format!(
                "Stashed {}",
                relative_time(self.stash.created_at).to_lowercase()
            )))
            .child(
                div()
                    .mt_3()
                    .px(rems(0.467))
                    .py_1()
                    .rounded(rems(0.333))
                    .border_1()
                    .border_color(palette.border_soft)
                    .bg(palette.bg)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .text_color(palette.text_soft)
                    .child(short_sha),
            )
            .child(
                div()
                    .w_full()
                    .child(FileTotals::new(self.count, self.totals)),
            )
    }
}
