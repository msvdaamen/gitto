//! A centered placeholder for an empty, loading or failed panel. Ported from
//! `components/ui/empty-state.tsx`.

use std::time::Duration;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, ElementId, FontWeight, Role, SharedString,
    Transformation, Window, div, percentage,
};

use super::icons::{Icon, IconName};
use crate::delayed::Delayed;
use crate::theme::PaletteExt as _;

/// How long something loads before [`LoadingState`] says so: a quick load doesn't flash it.
pub const LOADING_SHOWN_AFTER: Duration = Duration::from_millis(150);

/// A centered placeholder for an empty, loading or failed panel: an icon, a title, and what to
/// know about it. An error's message is in coral; a loading one's icon spins. What can be done
/// about it, like a button, goes under the message (its children).
#[derive(IntoElement)]
pub struct EmptyState {
    id: ElementId,
    icon: IconName,
    title: SharedString,
    message: Option<SharedString>,
    error: bool,
    loading: bool,
    fill: bool,
    children: Vec<AnyElement>,
}

impl ParentElement for EmptyState {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl EmptyState {
    pub fn new(id: impl Into<ElementId>, icon: IconName, title: impl Into<SharedString>) -> Self {
        EmptyState {
            id: id.into(),
            icon,
            title: title.into(),
            message: None,
            error: false,
            loading: false,
            fill: false,
            children: Vec::new(),
        }
    }

    /// What to know about it, under the title.
    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// The message is why something failed.
    pub fn error(mut self) -> Self {
        self.error = true;
        self
    }

    /// It's waiting for something to load: the icon spins, and it's announced as a status.
    pub fn loading(mut self) -> Self {
        self.loading = true;
        self
    }

    /// Fills its container, rather than taking the TS's fixed 250px.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let icon = Icon::new(self.icon).size_5();
        let icon = if self.loading {
            icon.with_animation(
                ElementId::from("spin"),
                Animation::new(Duration::from_secs(1)).repeat(),
                |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
            )
            .into_any_element()
        } else {
            icon.into_any_element()
        };
        v_flex()
            .id(self.id)
            .when(self.loading, |this| this.role(Role::Status))
            .when(self.fill, |this| this.size_full())
            .when(!self.fill, |this| this.h_64())
            .items_center()
            .justify_center()
            .gap_1p5()
            .px_6()
            .text_center()
            .text_color(palette.faint)
            .child(icon)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.text_soft)
                    .child(self.title),
            )
            .when_some(self.message, |this, message| {
                this.child(
                    div()
                        .text_xs()
                        .when(self.error, |this| this.text_color(palette.coral))
                        .child(message),
                )
            })
            .when(!self.children.is_empty(), |this| {
                this.child(div().mt_1p5().children(self.children))
            })
    }
}

/// An [`EmptyState`] saying `title` is loading, shown once that's taken a moment (see
/// [`LOADING_SHOWN_AFTER`]).
#[derive(IntoElement)]
pub struct LoadingState {
    id: ElementId,
    title: SharedString,
    fill: bool,
}

impl LoadingState {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        LoadingState {
            id: id.into(),
            title: title.into(),
            fill: false,
        }
    }

    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
}

impl RenderOnce for LoadingState {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Kept while it's on show: shown from the first frame it's rendered for that long.
        let shown = window.use_keyed_state(self.id.clone(), cx, |_, cx| {
            Delayed::new(true, LOADING_SHOWN_AFTER, cx)
        });
        let state = EmptyState::new(self.id, IconName::LoaderCircle, self.title)
            .loading()
            .when(self.fill, EmptyState::fill);
        div()
            .when(self.fill, |this| this.size_full())
            .when(shown.read(cx).is_on(), |this| this.child(state))
    }
}
