//! Icons: Lucide's, as `lucide-solid` gave them, and Gitto's own. Ported from the icons the
//! components imported, and `components/ui/gitto-icon.tsx`.
//!
//! [`IconName`] is the whole Lucide catalog, named as `lucide-solid` named them
//! (`lucide-solid/icons/git-branch` is `IconName::GitBranch`); `crate::assets::AppAssets` serves
//! them all. Draw one with GPUI Kit's `Icon`: `Icon::new(IconName::GitBranch).size_3()`.

use gpui_kit::prelude::*;
use gpui_kit::{App, ObjectFit, Window, img};

pub use gpui_kit::assets::IconName;
pub use gpui_kit::component::Icon;

/// Gitto's logo: its blob, smiling (`GittoIcon`). In color, as an image: it's art, not a glyph.
/// Sized by its owner, e.g. `.w_6().h_5()`.
#[derive(IntoElement)]
pub struct GittoIcon {
    style: gpui_kit::StyleRefinement,
}

impl GittoIcon {
    pub fn new() -> Self {
        GittoIcon {
            style: gpui_kit::StyleRefinement::default(),
        }
    }
}

impl Default for GittoIcon {
    fn default() -> Self {
        GittoIcon::new()
    }
}

impl Styled for GittoIcon {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for GittoIcon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut image = img("brand/gitto.svg")
            .object_fit(ObjectFit::Contain)
            .flex_none();
        image.style().refine(&self.style);
        image
    }
}
