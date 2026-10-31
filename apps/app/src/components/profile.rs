//! The user's avatar, and its menu. Ported from `components/profile.tsx`.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, SharedString, Window, div};

use crate::components::ui::{Icon, IconName};
use crate::state::router::{Route, Router};
use crate::theme::{AppTheme, PaletteExt as _};
use crate::util::format;

/// The user's avatar, which opens a menu of what's Gitto's own rather than a repository's: the
/// theme, and the settings. It shows the initials of the name the user commits with, or a person
/// without one. (TS hard-coded its initials.)
#[derive(IntoElement, Default)]
pub struct Profile {
    initials: Option<SharedString>,
}

impl Profile {
    /// The avatar of the user named `name`, if they've set one.
    pub fn new(name: Option<&str>) -> Self {
        Profile {
            initials: name.map(|name| format::initials(name).into()),
        }
    }
}

impl RenderOnce for Profile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        Button::new("profile")
            .ghost()
            .rounded(ButtonRounded::Size(cx.theme().radius_full()))
            .accessibility_label("Open profile")
            .child(
                div()
                    .flex()
                    .size_7()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(gpui_kit::linear_gradient(
                        145.,
                        gpui_kit::linear_color_stop(palette.brand_start, 0.),
                        gpui_kit::linear_color_stop(palette.blue, 1.),
                    ))
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette.avatar_text)
                    .map(|this| match self.initials {
                        Some(initials) => this.child(initials),
                        None => this.child(Icon::new(IconName::User).size_4()),
                    }),
            )
            .dropdown_menu(|menu, window, cx| {
                let dark = cx.app_theme() == AppTheme::Dark;
                // `min-w-[180px]`.
                menu.min_w(gpui_kit::rems(12.).to_pixels(window.rem_size()))
                    .item(
                        PopupMenuItem::new(if dark { "Light theme" } else { "Dark theme" })
                            .icon(Icon::new(if dark { IconName::Sun } else { IconName::Moon }))
                            .on_click(|_, _, cx| crate::theme::toggle(cx)),
                    )
                    .item(
                        PopupMenuItem::new("Settings")
                            .icon(Icon::new(IconName::Settings))
                            .on_click(|_, _, cx| Router::navigate(Route::Settings, cx)),
                    )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_the_initials_of_the_users_name() {
        assert_eq!(
            Profile::new(Some("Ada Lovelace")).initials.as_deref(),
            Some("AL")
        );
        assert_eq!(Profile::new(None).initials, None);
    }
}
