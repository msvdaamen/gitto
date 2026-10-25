//! A modal that asks before going on. Ported from `components/ui/confirm-dialog.tsx`.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::prelude::*;
use gpui_kit::{App, SharedString, Window, div};

use super::dialog::{CONFIRM_WIDTH, DialogHeading};
use super::icons::IconName;
use super::tone::Tone;

type Action = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// Asks before going on with something, like deleting a stash: what it's about, with an icon in
/// its tone (amber for something to think twice about, coral for something that's lost), and the
/// buttons to answer with, Cancel first. Escape, or clicking outside, cancels.
///
/// ```ignore
/// ConfirmDialog::new(IconName::X, Tone::Amber, format!("Remove {name}?"))
///     .description("Its tab is closed…")
///     .action("Remove", ButtonVariant::Primary, move |window, cx| remove(…))
///     .open(window, cx);
/// ```
pub struct ConfirmDialog {
    icon: IconName,
    tone: Tone,
    title: SharedString,
    description: Option<SharedString>,
    actions: Vec<(SharedString, ButtonVariant, Action)>,
    on_cancel: Option<Action>,
}

impl ConfirmDialog {
    pub fn new(icon: IconName, tone: Tone, title: impl Into<SharedString>) -> Self {
        ConfirmDialog {
            icon,
            tone,
            title: title.into(),
            description: None,
            actions: Vec::new(),
            on_cancel: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// A button after Cancel, which closes the dialog and runs `run`. The last one added is the
    /// default, its variant `Primary` (or `Danger` for something lost) by convention.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        variant: ButtonVariant,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions.push((label.into(), variant, Rc::new(run)));
        self
    }

    /// Told when it's cancelled: Cancel, Escape, or a click outside.
    pub fn on_cancel(mut self, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(run));
        self
    }

    /// Opens it over the window.
    pub fn open(self, window: &mut Window, cx: &mut App) {
        let this = Rc::new(self);
        window.open_dialog(cx, move |dialog, window, _| {
            let this = Rc::clone(&this);
            let on_cancel = this.on_cancel.clone();
            let mut footer = DialogFooter::new().gap_2().child(
                Button::new("cancel").ghost().label("Cancel").on_click({
                    let on_cancel = on_cancel.clone();
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        if let Some(on_cancel) = &on_cancel {
                            on_cancel(window, cx);
                        }
                    }
                }),
            );
            for (ix, (label, variant, run)) in this.actions.iter().enumerate() {
                let run = Rc::clone(run);
                footer = footer.child(
                    Button::new(("action", ix))
                        .with_variant(*variant)
                        .label(label.clone())
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            run(window, cx);
                        }),
                );
            }
            let heading = DialogHeading::new(this.icon, this.tone, this.title.clone())
                .when_some(this.description.clone(), |heading, description| {
                    heading.description(div().child(description))
                });
            dialog
                .w(CONFIRM_WIDTH.to_pixels(window.rem_size()))
                .close_button(false)
                .on_cancel(move |_, window, cx| {
                    if let Some(on_cancel) = &on_cancel {
                        on_cancel(window, cx);
                    }
                    true
                })
                .child(heading)
                .footer(footer)
        });
    }
}
