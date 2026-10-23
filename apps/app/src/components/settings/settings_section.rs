//! **Stub.** A group of settings. Replaces `components/settings/settings-section.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, SharedString, Window, div};

use crate::components::placeholder::Placeholder;
use crate::components::ui::{Icon, IconName};

/// A group of settings on the settings page, like Privacy: its title, what it's about, and its
/// settings (its children), each in a row of its own. Stub: renders a placeholder above them.
#[derive(IntoElement)]
pub struct SettingsSection {
    icon: IconName,
    title: SharedString,
    description: SharedString,
    children: Vec<AnyElement>,
}

impl SettingsSection {
    pub fn new(
        icon: IconName,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        SettingsSection {
            icon,
            title: title.into(),
            description: description.into(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for SettingsSection {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for SettingsSection {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Icon::new(self.icon).size_4())
            .child(
                div().h_16().child(
                    Placeholder::new(
                        "Settings section",
                        "components/settings/settings-section.tsx",
                    )
                    .detail(format!("{}: {}", self.title, self.description)),
                ),
            )
            .children(self.children)
    }
}
