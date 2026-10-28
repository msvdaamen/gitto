//! A conflict resolved by keeping one side whole. Ported from `conflict-sides.tsx`.

use std::rc::Rc;
use std::sync::Arc;

use gitto_git::features::conflicts::schema::{Conflict, Side};
use gitto_ui_logic::git::conflicts::{
    describe_conflict, describe_side, keep_label, unreadable_reason,
};
use gpui_kit::component::button::Button;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, Window, div, rems};

use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{Icon, IconName};

type OnKeep = Rc<dyn Fn(Side, &mut Window, &mut App) + 'static>;

/// A conflict that's resolved by keeping one side whole: what each side has at the file's path,
/// and a button to keep it, e.g. "Delete, as theirs does".
#[derive(IntoElement)]
pub struct ConflictSides {
    conflict: Arc<Conflict>,
    /// Why the file isn't shown as text, if there's more to say than the sides do.
    unreadable: Option<String>,
    disabled: bool,
    on_keep: OnKeep,
}

impl ConflictSides {
    /// The sides of `conflict`, the conflicted file at `path`.
    pub fn new(
        conflict: Arc<Conflict>,
        path: &str,
        disabled: bool,
        on_keep: impl Fn(Side, &mut Window, &mut App) + 'static,
    ) -> Self {
        ConflictSides {
            unreadable: unreadable_reason(&conflict.file, path),
            conflict,
            disabled,
            on_keep: Rc::new(on_keep),
        }
    }
}

impl RenderOnce for ConflictSides {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let sides = &self.conflict.sides;
        let side = |side: Side| {
            let (name, id, of) = match side {
                Side::Ours => ("ours", "keep-ours", &sides.ours),
                Side::Theirs => ("theirs", "keep-theirs", &sides.theirs),
            };
            let on_keep = Rc::clone(&self.on_keep);
            v_flex()
                .flex_1()
                .items_center()
                .gap_2()
                .rounded_lg()
                .border_1()
                .border_color(palette.border)
                .bg(palette.panel)
                .p_3()
                .child(
                    div()
                        .text_size(rems(0.7))
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette.muted)
                        .child(name.to_uppercase()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(palette.text_soft)
                        .child(describe_side(of.as_ref())),
                )
                .child(
                    Button::new(id)
                        .small()
                        .label(keep_label(sides, side))
                        .disabled(self.disabled)
                        .on_click(move |_, window, cx| on_keep(side, window, cx)),
                )
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_4()
            .px_6()
            .child(
                v_flex()
                    .items_center()
                    .gap_1p5()
                    .text_color(palette.faint)
                    .child(Icon::new(IconName::GitMergeConflict).size_5())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.text_soft)
                            .child(describe_conflict(sides)),
                    )
                    .when_some(self.unreadable, |this, reason| {
                        this.child(div().text_xs().child(reason))
                    }),
            )
            .child(
                h_flex()
                    .w_full()
                    .max_w(rems(30.7))
                    .items_stretch()
                    .gap_3()
                    .child(side(Side::Ours))
                    .child(side(Side::Theirs)),
            )
    }
}
