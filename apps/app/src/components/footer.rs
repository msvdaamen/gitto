//! The window's footer. Ported from `components/footer.tsx`.

use gitto_git::features::status::schema::StatusSummary;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, Subscription, Window, div, rems};

use super::app_version::AppVersion;
use crate::components::ui::{Divider, GittoIcon, Icon, IconName, StatusDot};
use crate::data::{Part, Repositories, Repository, observe_selected};
use crate::state::router::Router;
use crate::state::update::UpdateStatus;
use crate::theme::PaletteExt as _;
use gitto_ui_logic::git::status::{head_label, sync_label};

/// The footer's height (TS's grid row, 27px).
pub const FOOTER_HEIGHT: f32 = 1.8;

/// What's going on: ready, the open repository's branch and how it compares to its upstream, and
/// which Gitto this is.
pub struct Footer {
    /// The open repository, and the subscription that renders its status's summary.
    status: Option<(Entity<Repository>, Subscription)>,
    update: Entity<UpdateStatus>,
    _subscriptions: Vec<Subscription>,
}

impl Footer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let router = Router::global(cx);
        let update = UpdateStatus::global(cx);
        let subscriptions = vec![
            cx.observe(&router, |footer, _, cx| footer.follow_route(cx)),
            cx.observe(&update, |_, _, cx| cx.notify()),
        ];
        let mut footer = Footer {
            status: None,
            update,
            _subscriptions: subscriptions,
        };
        footer.follow_route(cx);
        footer
    }

    /// Shows the open repository's status, if a repository's open.
    fn follow_route(&mut self, cx: &mut Context<Self>) {
        let route = Router::current(cx);
        let id = route.repository_id();
        if self.status.as_ref().map(|(shown, _)| shown.read(cx).id()) == id {
            return;
        }
        self.status = id.map(|id| {
            let repository = Repositories::open(id, cx);
            repository.update(cx, |repository, cx| repository.show(Part::Status, cx));
            let subscription = observe_selected(&repository, Part::Status, summary, cx);
            (repository, subscription)
        });
        cx.notify();
    }

    fn render_repository_status(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let status = summary(self.status.as_ref()?.0.read(cx))?;
        let palette = *cx.palette();
        Some(
            h_flex()
                .gap_1()
                .child(Divider::new())
                .child(
                    h_flex()
                        .gap_1()
                        .child(Icon::new(IconName::GitBranch).size_3())
                        .child(head_label(&status.head).to_owned()),
                )
                .when_some(status.upstream.clone(), |this, upstream| {
                    this.child(
                        h_flex()
                            .gap_1()
                            .text_color(palette.blue)
                            .child(Icon::new(IconName::Cloud).size_3())
                            .child(format!(
                                "{upstream} · {}",
                                sync_label(status.ahead, status.behind)
                            )),
                    )
                }),
        )
    }
}

/// What the footer shows of a repository's status.
fn summary(repository: &Repository) -> Option<StatusSummary> {
    repository.status().data().map(|status| status.summary())
}

impl Render for Footer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .h(rems(FOOTER_HEIGHT))
            .flex_none()
            .justify_between()
            .gap_5()
            .border_t_1()
            .border_color(palette.border)
            .bg(palette.footer)
            .px_3()
            .text_sm()
            .text_color(palette.muted)
            .child(
                h_flex()
                    .gap_1()
                    .whitespace_nowrap()
                    .child(GittoIcon::new().w_3p5().h_3())
                    .child(StatusDot::new(palette.mint))
                    .child(div().child("Ready"))
                    .children(self.render_repository_status(cx)),
            )
            .child(AppVersion::new(self.update.read(cx).state().cloned()))
    }
}
