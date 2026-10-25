//! A one-line text input. Ported from `components/ui/text-input.tsx`.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, Window};

use super::icons::{Icon, IconName};
use crate::theme::PaletteExt as _;

/// GPUI Kit's `Input` for the state its owner keeps (an `Entity<InputState>`, whose events it
/// subscribes to), with an icon before the text, e.g. a search box's. `compact` is the small one
/// a toolbar or a list's header has.
#[derive(IntoElement)]
pub struct TextInput {
    state: Entity<InputState>,
    icon: Option<IconName>,
    compact: bool,
}

impl TextInput {
    pub fn new(state: &Entity<InputState>) -> Self {
        TextInput {
            state: state.clone(),
            icon: None,
            compact: false,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }
}

impl RenderOnce for TextInput {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        Input::new(&self.state)
            .when(self.compact, |input| input.small().w_56())
            .when(!self.compact, |input| input.w_56())
            .when_some(self.icon, |input, icon| {
                input.prefix(Icon::new(icon).size_4().text_color(palette.muted))
            })
    }
}
