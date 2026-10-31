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
/// buttons to answer with, Cancel first. Escape, Enter, or clicking outside, cancels: as in TS,
/// where Cancel has the focus when it opens, Enter doesn't go on with something unasked.
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
    /// main one, its variant `Primary` (or `Danger` for something lost) by convention.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        variant: ButtonVariant,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.actions.push((label.into(), variant, Rc::new(run)));
        self
    }

    /// Told when it's cancelled: Cancel, Escape, Enter, or a click outside.
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
                // Kit's dialog takes Enter as OK, closing it without a word otherwise.
                .on_ok({
                    let on_cancel = on_cancel.clone();
                    move |_, window, cx| {
                        if let Some(on_cancel) = &on_cancel {
                            on_cancel(window, cx);
                        }
                        true
                    }
                })
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Context, IntoElement, Render, TestAppContext};

    use super::*;
    use crate::test_support;

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// How many times it was removed, and cancelled.
    #[derive(Default)]
    struct Counts {
        removed: Rc<Cell<u32>>,
        cancelled: Rc<Cell<u32>>,
    }

    /// A dialog asking to remove something, counting the removes and the cancels.
    fn open(cx: &mut TestAppContext) -> (tempfile::TempDir, AnyWindowHandle, Counts) {
        let dir = test_support::init(cx, |services| services);
        let (window, _) = test_support::open(cx, |_, _| Host);
        let counts = Counts::default();
        let (removed, cancelled) = (Rc::clone(&counts.removed), Rc::clone(&counts.cancelled));
        cx.update_window(window, |_, window, cx| {
            ConfirmDialog::new(IconName::X, Tone::Amber, "Remove it?")
                .action("Remove", ButtonVariant::Primary, move |_, _| {
                    removed.set(removed.get() + 1)
                })
                .on_cancel(move |_, _| cancelled.set(cancelled.get() + 1))
                .open(window, cx);
        })
        .unwrap();
        cx.run_until_parked();
        (dir, window, counts)
    }

    fn has_dialog(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    #[gpui_kit::test]
    fn cancels_on_enter(cx: &mut TestAppContext) {
        let (_dir, window, counts) = open(cx);
        assert!(has_dialog(window, cx));
        cx.update_window(window, |_, window, cx| window.press("enter", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(!has_dialog(window, cx));
        assert_eq!((counts.removed.get(), counts.cancelled.get()), (0, 1));
    }

    #[gpui_kit::test]
    fn runs_the_action_chosen(cx: &mut TestAppContext) {
        let (_dir, window, counts) = open(cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click(("action", 0usize), cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(!has_dialog(window, cx));
        assert_eq!((counts.removed.get(), counts.cancelled.get()), (1, 0));
    }
}
