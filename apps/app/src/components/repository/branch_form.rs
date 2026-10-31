//! The form that names a new branch. Ported from `components/repository/branch-form.tsx`.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{Disableable as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, FontWeight, SharedString, Window, div};

use crate::components::ui::{FormField, Icon, IconName};
use crate::theme::PaletteExt as _;

/// The name field's placeholder: made with the field's `InputState`.
pub const NAME_PLACEHOLDER: &str = "feature/my-change";

type OnSubmit = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// The name typed in `name`, trimmed, if it's one to submit: not empty.
pub fn submitted_name(name: &Entity<InputState>, cx: &App) -> Option<String> {
    let trimmed = name.read(cx).value().trim().to_owned();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// Whether a name to submit is typed in `name`.
fn is_named(name: &Entity<InputState>, cx: &App) -> bool {
    !name.read(cx).value().trim().is_empty()
}

/// The form that names a new branch: its title, what it's made from (`summary`), the name, and
/// Create branch, which submits it. It's in a popover (the toolbar's Branch) or a dialog (a
/// branch's Create branch…). The owner keeps the name's `InputState` (with [`NAME_PLACEHOLDER`]),
/// and submits on Enter in it too, through [`submitted_name`].
#[derive(IntoElement)]
pub struct BranchForm {
    /// What it's made from, and what comes along, e.g. "From main.".
    summary: SharedString,
    name: Entity<InputState>,
    /// Can't be submitted for now, e.g. while something it'd wait for is running.
    disabled: bool,
    on_submit: Option<OnSubmit>,
}

impl BranchForm {
    pub fn new(summary: impl Into<SharedString>, name: &Entity<InputState>) -> Self {
        BranchForm {
            summary: summary.into(),
            name: name.clone(),
            disabled: false,
            on_submit: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called once the name isn't empty, unless it's disabled: the owner reads it with
    /// [`submitted_name`].
    pub fn on_submit(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for BranchForm {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let named = is_named(&self.name, cx);
        let name = self.name.clone();
        let on_submit = self.on_submit;
        v_flex()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.text)
                    .child("New branch"),
            )
            .child(
                div()
                    .mt_1()
                    .mb_2p5()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(self.summary),
            )
            .child(FormField::input("Name", &self.name))
            .child(
                Button::new("create-branch")
                    .primary()
                    .w_full()
                    .icon(Icon::new(IconName::GitBranch))
                    .label("Create branch")
                    .disabled(!named || self.disabled)
                    .on_click(move |_, window, cx| {
                        if let Some(on_submit) = &on_submit
                            && is_named(&name, cx)
                        {
                            on_submit(window, cx);
                        }
                    }),
            )
    }
}
