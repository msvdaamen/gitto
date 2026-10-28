//! The window's header. Ported from `components/header.tsx`.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Window, rems};

use super::notifications::Notifications;
use super::profile::Profile;
use super::tabs::Tabs;
use crate::data::Query;
use crate::data::user::user_name_query;
use crate::theme::PaletteExt as _;

/// The header's height (TS's grid row, 49px).
pub const HEADER_HEIGHT: f32 = 3.25;

/// The tabs, then the bell and the profile on the right.
pub struct Header {
    tabs: Entity<Tabs>,
    notifications: Entity<Notifications>,
    /// The name the user commits with, which the profile shows the initials of.
    user_name: Query<Option<String>>,
}

impl Header {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Header {
            tabs: cx.new(Tabs::new),
            notifications: cx.new(Notifications::new),
            user_name: user_name_query(cx),
        }
    }

    pub fn tabs(&self) -> &Entity<Tabs> {
        &self.tabs
    }
}

impl Render for Header {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
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
                    .child(Profile::new(self.user_name.data().flatten().as_deref())),
            )
    }
}
