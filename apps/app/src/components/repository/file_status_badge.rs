//! A file's status as a letter. Ported from `components/ui/file-status-badge.tsx`: a primitive
//! there, but one of git's file statuses, which the app's primitives (`gitto-app-kit`) know nothing
//! of.

use gitto_git::FileStatus;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, FontWeight, Window, div};

use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::Tone;

/// The letter for a status, like `M` for modified.
pub fn file_status_label(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Modified => "M",
        FileStatus::Added => "A",
        FileStatus::Deleted => "D",
        FileStatus::Renamed => "R",
        FileStatus::Copied => "C",
        FileStatus::Typechange => "T",
        FileStatus::Untracked => "U",
        FileStatus::Conflicted => "!",
    }
}

/// The tone for a status.
pub fn file_status_tone(status: FileStatus) -> Tone {
    match status {
        FileStatus::Modified | FileStatus::Typechange => Tone::Amber,
        FileStatus::Added | FileStatus::Untracked => Tone::Mint,
        FileStatus::Deleted | FileStatus::Conflicted => Tone::Coral,
        FileStatus::Renamed | FileStatus::Copied => Tone::Blue,
    }
}

/// A file's status as a letter, like `M` for modified, in its colour. A conflicted file that's
/// `ready`, with no conflict markers left, has a check instead, until it's marked resolved.
#[derive(IntoElement)]
pub struct FileStatusBadge {
    id: ElementId,
    status: FileStatus,
    ready: bool,
}

impl FileStatusBadge {
    pub fn new(id: impl Into<ElementId>, status: FileStatus) -> Self {
        FileStatusBadge {
            id: id.into(),
            status,
            ready: false,
        }
    }

    pub fn ready(mut self, ready: bool) -> Self {
        self.ready = ready;
        self
    }
}

impl RenderOnce for FileStatusBadge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let ready = self.status == FileStatus::Conflicted && self.ready;
        let tone = if ready {
            Tone::Mint
        } else {
            file_status_tone(self.status)
        };
        let (background, foreground) = tone.colors(cx.palette());
        let title = if ready {
            "conflicted, but resolved: ready to mark resolved"
        } else {
            self.status.as_str()
        };
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .size_4()
            .items_center()
            .justify_center()
            .rounded_sm()
            .bg(background)
            .text_color(foreground)
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx)
            })
            .child(if ready {
                "✓"
            } else {
                file_status_label(self.status)
            })
    }
}
