//! A line under a header about something that went wrong. Ported from
//! `components/ui/notice.tsx`.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Role, SharedString, Window, div};

use super::icons::{Icon, IconName};
use crate::theme::PaletteExt as _;

/// A line under a header about something that went wrong, like a file's edits that couldn't be
/// saved. A status by default: the user is told, but not interrupted; an alert for what they
/// asked for and didn't get, like lines that couldn't be staged. Buttons for what can be done
/// about it go after the message (its children).
#[derive(IntoElement)]
pub struct Notice {
    message: SharedString,
    alert: bool,
    icon: IconName,
    children: Vec<AnyElement>,
}

impl Notice {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Notice {
            message: message.into(),
            alert: false,
            icon: IconName::FileExclamationPoint,
            children: Vec::new(),
        }
    }

    /// Announced as an alert rather than a status.
    pub fn alert(mut self) -> Self {
        self.alert = true;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = icon;
        self
    }
}

impl ParentElement for Notice {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Notice {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let has_actions = !self.children.is_empty();
        h_flex()
            .id("notice")
            .role(if self.alert {
                Role::Alert
            } else {
                Role::Status
            })
            .flex_none()
            .gap_1p5()
            .border_b_1()
            .border_color(palette.border)
            .px_3()
            // Less padding around buttons, which are taller than the text: the same height either
            // way.
            .when(has_actions, |this| this.py_1())
            .when(!has_actions, |this| this.py_1p5())
            .text_xs()
            .text_color(palette.muted)
            .child(Icon::new(self.icon).size_3().text_color(palette.amber))
            .child(div().mr_auto().child(self.message))
            .children(self.children)
    }
}
