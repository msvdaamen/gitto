//! Sections shared by the details of a commit and a stash. Ported from
//! `components/repository/details/details-sections.tsx`.

use std::rc::Rc;

use gitto_git::features::diff::schema::ChangedFile;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, ListAlignment, ListState, SharedString, Window, div,
    list, px, rems,
};

use super::changed_file_list::{ChangedFileList, ROW_PITCH};
use crate::components::ui::{Icon, IconName, LineStats, SectionHeader};
use crate::data::{DataError, Opaque};
use crate::theme::PaletteExt as _;
use gitto_ui::git::changes::LineCounts;

/// Why some of the details couldn't be loaded, e.g. "Couldn't load the commit", above the rest;
/// nothing without an error.
#[derive(IntoElement)]
pub struct DetailsError {
    id: ElementId,
    title: SharedString,
    error: Option<DataError>,
}

impl DetailsError {
    pub fn new(
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        error: Option<DataError>,
    ) -> Self {
        DetailsError {
            id: id.into(),
            title: title.into(),
            error,
        }
    }
}

impl RenderOnce for DetailsError {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        div().when_some(self.error, |this, error| {
            this.child(
                v_flex()
                    .id(self.id)
                    .flex_none()
                    .items_center()
                    .gap_1p5()
                    .px_6()
                    .py_4()
                    .border_b_1()
                    .border_color(palette.border)
                    .text_center()
                    .text_color(palette.faint)
                    .child(Icon::new(IconName::TriangleAlert).size_5())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.text_soft)
                            .child(self.title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.coral)
                            .child(error.message()),
                    ),
            )
        })
    }
}

/// How many files changed, and the lines added and removed, at the bottom of a summary.
#[derive(IntoElement)]
pub struct FileTotals {
    count: usize,
    totals: LineCounts,
}

impl FileTotals {
    pub fn new(count: usize, totals: LineCounts) -> Self {
        FileTotals { count, totals }
    }
}

impl RenderOnce for FileTotals {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .mt_3()
            .gap_2()
            .text_xs()
            .text_color(palette.muted)
            .child(
                h_flex()
                    .mr_auto()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.text_soft)
                            .child(self.count.to_string()),
                    )
                    .child("files changed"),
            )
            .child(LineStats::new(self.totals.additions, self.totals.deletions))
    }
}

/// The heading of a commit's or stash's changed files, with how many there are.
pub fn changed_files_heading(count: usize) -> impl IntoElement {
    div()
        .px_1()
        .pb_2()
        .child(SectionHeader::new(IconName::File, "Changed files").count(count as u64))
}

/// What's above the files in a [`DetailsList`], rendered when it's in view.
pub type RenderTop = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// Items before the files' rows: what's above them, and their heading.
const ROWS_START: usize = 2;

/// What a commit's or stash's details show, scrolled as one: what's above the files (the top),
/// then the files under their heading. Only the rows in view are rendered (GPUI's `list`, which
/// measures each item, the one above the files being as tall as its message): a commit can change
/// tens of thousands of files. Kept by the details' entity across renders, for its scroll position.
pub struct DetailsList {
    state: ListState,
    /// The files laid out, and the key of what's above them: another list of files is laid out
    /// again from the top; another top is measured again.
    shape: Option<(Opaque<Vec<ChangedFile>>, u64)>,
}

impl DetailsList {
    pub fn new() -> Self {
        DetailsList {
            state: ListState::new(ROWS_START, ListAlignment::Top, px(200.)),
            shape: None,
        }
    }

    /// The list `id`, of `top` (whose look `top_key` tells), and `files`.
    pub fn render(
        &mut self,
        id: &'static str,
        top: RenderTop,
        top_key: u64,
        files: ChangedFileList,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let palette = *cx.palette();
        let shown = files.files().clone();
        match &self.shape {
            // No files is the same shape whichever empty list it's told: its owner makes another
            // on each render while there are none.
            Some((last, key)) if *last == shown || (last.is_empty() && shown.is_empty()) => {
                if *key != top_key {
                    self.state.remeasure_items(0..1);
                }
            }
            _ => self.state.reset(ROWS_START + files.len()),
        }
        self.shape = Some((shown, top_key));
        let count = files.len();
        div().id(id).size_full().child(
            list(self.state.clone(), move |ix, window, cx| match ix {
                0 => top(window, cx),
                1 => div()
                    .px_2p5()
                    .pt_3()
                    .child(changed_files_heading(count))
                    .into_any_element(),
                ix => {
                    let row = ix - ROWS_START;
                    div()
                        .px_2p5()
                        .h(ROW_PITCH)
                        .when(row + 1 == count, |this| {
                            this.h(ROW_PITCH + rems(0.8))
                                .border_b_1()
                                .border_color(palette.border)
                        })
                        .child(files.row(row, cx))
                        .into_any_element()
                }
            })
            .size_full(),
        )
    }
}

impl Default for DetailsList {
    fn default() -> Self {
        DetailsList::new()
    }
}
