//! Dialogs. Ported from `components/ui/dialog.tsx`.
//!
//! A dialog is GPUI Kit's, opened on the window with [`DialogId::open`], which is Kit's
//! `window.open_dialog(cx, |dialog, window, cx| …)` (`WindowExt`) with a name to close it by. The
//! window's `Root` draws it over the app, dimmed behind it, traps focus in it, closes it on Escape
//! or a click outside, and gives focus back after. What TS's `DIALOG_BOX` and `DialogPortal` did
//! is the theme's (its `popover`, `overlay` and `radius_lg`).
//!
//! Gitto's dialogs start with a [`DialogHeading`]: an icon in a tone, the title, and what to know.
//! Build what it's about before opening it, and keep the builder cheap: it runs again whenever the
//! dialog layer is drawn.

use std::collections::HashMap;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, FontWeight, Global, Rems, SharedString, Window, div,
};

use super::icons::{Icon, IconName};
use super::tone::Tone;
use crate::theme::PaletteExt as _;

/// How wide a short confirmation is (`max-w-[440px]`), in rems of the 15px base.
pub const CONFIRM_WIDTH: Rems = Rems(29.33);
/// How wide a dialog with a list is, like What's new (`max-w-[520px]`).
pub const LIST_WIDTH: Rems = Rems(34.67);

/// A dialog opened with [`DialogId::open`], to close it by: Kit's `window.close_dialog` closes
/// whichever is on top, which needn't be the one its owner means once another opens over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DialogId(u64);

/// The dialogs opened with [`DialogId::open`] in each window, bottom to top as Kit stacks them,
/// each with whether it's been closed under another: it goes once it's on top.
#[derive(Default)]
struct Dialogs {
    last: u64,
    windows: HashMap<AnyWindowHandle, Vec<(DialogId, bool)>>,
}

impl Global for Dialogs {}

impl DialogId {
    /// Opens a dialog over the window, as Kit's `window.open_dialog` does, `build` given its id
    /// too. Every dialog opens this way, so that each closes as its own: Kit closes the top one on
    /// Escape, a click outside, or Enter (when `on_ok` says so), and `build` mustn't set
    /// `on_close`, which keeps track of that.
    pub fn open(
        window: &mut Window,
        cx: &mut App,
        build: impl Fn(DialogId, Dialog, &mut Window, &mut App) -> Dialog + 'static,
    ) -> DialogId {
        let dialogs = cx.default_global::<Dialogs>();
        dialogs.last += 1;
        let id = DialogId(dialogs.last);
        let handle = window.window_handle();
        dialogs.windows.entry(handle).or_default().push((id, false));
        window.open_dialog(cx, move |dialog, window, cx| {
            build(id, dialog, window, cx).on_close(move |_, window, cx| {
                // Kit closed it, from the top.
                id.forget(window, cx);
                close_uncovered(window, cx);
            })
        });
        id
    }

    /// Closes this dialog, if it's open: at once if it's on top; otherwise once the ones over it
    /// have closed, as Kit only closes the top one.
    pub fn close(self, window: &mut Window, cx: &mut App) {
        let Some(stack) = cx
            .try_global::<Dialogs>()
            .and_then(|dialogs| dialogs.windows.get(&window.window_handle()))
        else {
            return;
        };
        match stack.iter().position(|(id, _)| *id == self) {
            Some(ix) if ix + 1 == stack.len() => {
                self.forget(window, cx);
                window.close_dialog(cx);
                close_uncovered(window, cx);
            }
            Some(ix) => {
                let handle = window.window_handle();
                if let Some(stack) = cx.global_mut::<Dialogs>().windows.get_mut(&handle) {
                    stack[ix].1 = true;
                }
            }
            None => {}
        }
    }

    /// Whether it's open, in `window`.
    pub fn is_open(self, window: &Window, cx: &App) -> bool {
        cx.try_global::<Dialogs>()
            .and_then(|dialogs| dialogs.windows.get(&window.window_handle()))
            .is_some_and(|stack| stack.iter().any(|(id, _)| *id == self))
    }

    /// Takes it off the window's stack.
    fn forget(self, window: &Window, cx: &mut App) {
        let handle = window.window_handle();
        let dialogs = cx.default_global::<Dialogs>();
        if let Some(stack) = dialogs.windows.get_mut(&handle) {
            stack.retain(|(id, _)| *id != self);
            if stack.is_empty() {
                dialogs.windows.remove(&handle);
            }
        }
    }
}

/// Closes the dialogs closed under another that are on top now.
fn close_uncovered(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    loop {
        let Some(&(id, true)) = cx
            .try_global::<Dialogs>()
            .and_then(|dialogs| dialogs.windows.get(&handle))
            .and_then(|stack| stack.last())
        else {
            return;
        };
        id.forget(window, cx);
        window.close_dialog(cx);
    }
}

/// A dialog's heading: its icon in a tone, in a rounded square, then its title, and under it what
/// to know (`description`), if anything.
#[derive(IntoElement)]
pub struct DialogHeading {
    icon: IconName,
    tone: Tone,
    title: SharedString,
    description: Vec<AnyElement>,
}

impl DialogHeading {
    pub fn new(icon: IconName, tone: Tone, title: impl Into<SharedString>) -> Self {
        DialogHeading {
            icon,
            tone,
            title: title.into(),
            description: Vec::new(),
        }
    }

    /// A paragraph under the title.
    pub fn description(mut self, paragraph: impl IntoElement) -> Self {
        self.description.push(paragraph.into_any_element());
        self
    }
}

impl RenderOnce for DialogHeading {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let (background, foreground) = self.tone.colors(&palette);
        h_flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size_9()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .bg(background)
                    .text_color(foreground)
                    .child(Icon::new(self.icon).size_5()),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(palette.text)
                            .child(self.title),
                    )
                    .when(!self.description.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .mt_2()
                                .gap_2()
                                .text_sm()
                                .text_color(palette.text_soft)
                                .children(self.description),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
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

    /// Two dialogs, one over the other.
    fn open_two(cx: &mut TestAppContext) -> (AnyWindowHandle, DialogId, DialogId) {
        test_support::init(cx);
        let (window, _) = test_support::open(cx, |_, _| Host);
        let (under, over) = cx
            .update_window(window, |_, window, cx| {
                let under = DialogId::open(window, cx, |_, dialog, _, _| dialog);
                let over = DialogId::open(window, cx, |_, dialog, _, _| dialog);
                (under, over)
            })
            .unwrap();
        cx.run_until_parked();
        (window, under, over)
    }

    fn is_open(window: AnyWindowHandle, id: DialogId, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| id.is_open(window, cx))
            .unwrap()
    }

    fn has_dialog(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    #[gpui_kit::test]
    fn closes_the_one_on_top_at_once(cx: &mut TestAppContext) {
        let (window, under, over) = open_two(cx);
        cx.update_window(window, |_, window, cx| over.close(window, cx))
            .unwrap();
        assert!(!is_open(window, over, cx));
        assert!(is_open(window, under, cx));
        assert!(has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn closes_one_under_another_once_thats_closed(cx: &mut TestAppContext) {
        let (window, under, over) = open_two(cx);
        cx.update_window(window, |_, window, cx| under.close(window, cx))
            .unwrap();
        // The one over it stays.
        assert!(is_open(window, over, cx));
        assert!(has_dialog(window, cx));

        // Closed by Kit, on Escape: the one under it goes too.
        cx.update_window(window, |_, window, cx| window.press("escape", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(!is_open(window, over, cx));
        assert!(!is_open(window, under, cx));
        assert!(!has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn knows_kit_closed_one(cx: &mut TestAppContext) {
        let (window, under, over) = open_two(cx);
        cx.update_window(window, |_, window, cx| window.press("escape", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(!is_open(window, over, cx));
        // So the one under it is on top, and closes at once.
        cx.update_window(window, |_, window, cx| under.close(window, cx))
            .unwrap();
        assert!(!has_dialog(window, cx));
    }
}
