//! A labelled text field for forms. Ported from `components/ui/form-field.tsx`.

use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, SharedString, Window, div, rems};

use crate::theme::PaletteExt as _;

enum Control {
    Input(Entity<InputState>),
    Textarea(Entity<TextareaState>),
}

/// A labelled text field for forms, with a hint by the label; a multi-line one for a
/// `TextareaState`. `readonly` keeps focus, unlike disabling it, e.g. to type the next message
/// once a commit is done.
#[derive(IntoElement)]
pub struct FormField {
    label: SharedString,
    hint: Option<SharedString>,
    control: Control,
    readonly: bool,
}

impl FormField {
    /// A one-line field for `state`.
    pub fn input(label: impl Into<SharedString>, state: &Entity<InputState>) -> Self {
        FormField {
            label: label.into(),
            hint: None,
            control: Control::Input(state.clone()),
            readonly: false,
        }
    }

    /// A multi-line field for `state`.
    pub fn textarea(label: impl Into<SharedString>, state: &Entity<TextareaState>) -> Self {
        FormField {
            label: label.into(),
            hint: None,
            control: Control::Textarea(state.clone()),
            readonly: false,
        }
    }

    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }
}

impl RenderOnce for FormField {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let control: AnyElement = match &self.control {
            Control::Input(state) => Input::new(state)
                .aria_label(self.label.clone())
                .readonly(self.readonly)
                .into_any_element(),
            Control::Textarea(state) => Textarea::new(state)
                .aria_label(self.label.clone())
                .readonly(self.readonly)
                .into_any_element(),
        };
        v_flex()
            .mb_2p5()
            .gap_1()
            .text_xs()
            .text_color(palette.muted)
            .child(
                h_flex()
                    .justify_between()
                    .child(div().child(self.label))
                    .when_some(self.hint, |this, hint| {
                        // A `<small>` in TS: smaller than the label.
                        this.child(
                            div()
                                .text_size(rems(10. / 15.))
                                .text_color(palette.faint)
                                .child(hint),
                        )
                    }),
            )
            .child(control)
    }
}
