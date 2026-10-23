//! Gitto's blob, as a friendly face on empty panels. Ported from `components/ui/mascot.tsx`.

use gpui_kit::prelude::*;
use gpui_kit::{App, ObjectFit, Pixels, Window, img};

/// Gitto's blob, its width `size` (and its height in proportion): a face for an empty panel. A
/// size in rems, from the scale, e.g. `Mascot::new(rems(3.))`.
#[derive(IntoElement)]
pub struct Mascot {
    size: gpui_kit::Rems,
}

impl Mascot {
    pub fn new(size: gpui_kit::Rems) -> Self {
        Mascot { size }
    }
}

impl RenderOnce for Mascot {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let width: Pixels = self.size.to_pixels(window.rem_size());
        img("brand/mascot.svg")
            .object_fit(ObjectFit::Contain)
            .flex_none()
            .w(width)
            .h(width * 0.85)
    }
}
