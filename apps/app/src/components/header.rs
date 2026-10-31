//! The window's header. Ported from `components/header.tsx`.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Subscription, Window, rems};

use super::notifications::Notifications;
use super::profile::Profile;
use super::tabs::Tabs;
use gitto_app_data::data::{AppChanged, AppData, AppPart};
use gitto_app_kit::theme::PaletteExt as _;

/// The header's height (TS's grid row, 49px).
pub const HEADER_HEIGHT: f32 = 3.25;

/// The tabs, then the bell and the profile on the right.
pub struct Header {
    tabs: Entity<Tabs>,
    notifications: Entity<Notifications>,
    /// What holds the name the user commits with, which the profile shows the initials of.
    data: Entity<AppData>,
    _user_name: Subscription,
}

impl Header {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let data = AppData::global(cx);
        data.update(cx, |data, cx| data.show(AppPart::UserName, cx));
        let user_name = cx.subscribe(&data, |_, _, AppChanged(part), cx| {
            if *part == AppPart::UserName {
                cx.notify();
            }
        });
        Header {
            tabs: cx.new(Tabs::new),
            notifications: cx.new(Notifications::new),
            data,
            _user_name: user_name,
        }
    }

    pub fn tabs(&self) -> &Entity<Tabs> {
        &self.tabs
    }
}

impl Render for Header {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        let user_name = self.data.read(cx).user_name().data().cloned().flatten();
        h_flex()
            .h(rems(HEADER_HEIGHT))
            .flex_none()
            .items_stretch()
            .min_w_0()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.header)
            .child(self.tabs.clone())
            .child(
                h_flex()
                    .ml_auto()
                    .min_w_0()
                    .gap_1p5()
                    .pl_2()
                    .pr_3()
                    .child(self.notifications.clone())
                    .child(Profile::new(user_name.as_deref())),
            )
    }
}
