//! A home page action that isn't built yet. Ported from `components/home/demo-action.ts`.

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, ClickEvent, SharedString, Window};

/// Click handler for a home page action that isn't built yet: says `message`, as a notification
/// rather than TS's `alert`.
pub fn demo_action(
    message: impl Into<SharedString>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let message = message.into();
    move |_, window, cx| window.push_notification(message.clone(), cx)
}
