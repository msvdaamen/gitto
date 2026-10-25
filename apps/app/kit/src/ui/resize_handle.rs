//! A draggable line on the edge of a panel that resizes it. Ported from
//! `components/ui/resize-handle.tsx`.
//!
//! The band is GPUI Kit's `resize_handle`: its hit area, cursor, hover and drag. What the drag
//! resizes is its owner's: the element the panel's edge is in (TS's handle's parent) follows the
//! drag with `.on_drag_move::<PanelDrag>` and asks [`dragged_width`] for the width at the
//! pointer, setting it without saving; the handle saves the width it's let go at (its
//! `on_resize`, with `save`):
//!
//! ```ignore
//! h_flex()
//!     .on_drag_move(cx.listener(|this, event: &DragMoveEvent<PanelDrag>, _, cx| {
//!         if let Some(width) = dragged_width(event, "sidebar", HandleSide::Start, cx) {
//!             this.sidebar.set_width(width, false);
//!         }
//!     }))
//! ```

use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Axis, DragMoveEvent, ElementId, Empty, Entity, FocusHandle, KeyDownEvent, MouseButton,
    Role, SharedString, Window,
};

use gitto_app_data::state::panel_width::PanelBounds;

/// How far an arrow key moves the edge, in pixels.
pub const KEYBOARD_STEP: f32 = 16.;

/// Which side of the edge its panel is on: `Start` for a panel on its left (the sidebar), `End`
/// for one on its right (the details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleSide {
    Start,
    End,
}

/// What a panel's handle drags: the panel, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelDrag {
    pub panel: &'static str,
}

/// Nothing follows the pointer: the panel's edge does.
impl Render for PanelDrag {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The width the panel `panel` takes with its edge at the pointer, measured in the element
/// listening (the edge's parent), if it's that panel that's dragged.
pub fn dragged_width(
    event: &DragMoveEvent<PanelDrag>,
    panel: &str,
    side: HandleSide,
    cx: &App,
) -> Option<f32> {
    if event.drag(cx).panel != panel {
        return None;
    }
    let x = event.event.position.x;
    let width = match side {
        HandleSide::Start => x - event.bounds.left(),
        HandleSide::End => event.bounds.right() - x,
    };
    Some(f32::from(width))
}

type OnResize = Rc<dyn Fn(f32, bool, &mut Window, &mut App) + 'static>;
type OnReset = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// A draggable line on the edge of a panel that resizes it (see the module's doc). Double-click
/// resets the width; with focus, the arrow keys nudge it, Home and End take it to its bounds.
/// `on_resize` is given the width to resize to, and whether to save it: always, from the
/// keyboard, and once a drag is let go of; not while it's dragged.
#[derive(IntoElement)]
pub struct ResizeHandle {
    id: ElementId,
    panel: &'static str,
    side: HandleSide,
    label: SharedString,
    width: f32,
    bounds: PanelBounds,
    on_resize: Option<OnResize>,
    on_reset: Option<OnReset>,
}

impl ResizeHandle {
    pub fn new(
        panel: &'static str,
        side: HandleSide,
        label: impl Into<SharedString>,
        width: f32,
        bounds: PanelBounds,
    ) -> Self {
        ResizeHandle {
            id: ElementId::Name(format!("resize-{panel}").into()),
            panel,
            side,
            label: label.into(),
            width,
            bounds,
            on_resize: None,
            on_reset: None,
        }
    }

    pub fn on_resize(
        mut self,
        handler: impl Fn(f32, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_resize = Some(Rc::new(handler));
        self
    }

    pub fn on_reset(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_reset = Some(Rc::new(handler));
        self
    }
}

/// The width a key takes the panel to, from `width`; `None` for a key that doesn't move it.
pub fn width_for_key(key: &str, side: HandleSide, width: f32, bounds: PanelBounds) -> Option<f32> {
    let (grow, shrink) = match side {
        HandleSide::Start => ("right", "left"),
        HandleSide::End => ("left", "right"),
    };
    match key {
        key if key == grow => Some(width + KEYBOARD_STEP),
        key if key == shrink => Some(width - KEYBOARD_STEP),
        "home" => Some(bounds.min),
        "end" => Some(bounds.max),
        _ => None,
    }
}

impl RenderOnce for ResizeHandle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus: FocusHandle = window
            .use_keyed_state((self.id.clone(), "focus"), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        // Whether the band's being dragged: its width is saved once it's let go of.
        let dragging = window.use_keyed_state((self.id.clone(), "dragging"), cx, |_, _| false);
        let panel = self.panel;
        let side = self.side;
        let width = self.width;
        let bounds = self.bounds;
        let on_resize = self.on_resize;
        let on_reset = self.on_reset;
        gpui_kit::component::resize_handle::<PanelDrag, PanelDrag>(self.id, Axis::Horizontal)
            .on_drag(PanelDrag { panel }, {
                let dragging = dragging.clone();
                move |drag, _, _, cx| {
                    dragging.update(cx, |dragging, _| *dragging = true);
                    cx.new(|_| drag.clone())
                }
            })
            .track_focus(&focus)
            .role(Role::Splitter)
            .aria_label(self.label)
            .aria_numeric_value(f64::from(width))
            .on_key_down({
                let on_resize = on_resize.clone();
                move |event: &KeyDownEvent, window, cx| {
                    let Some(next) = width_for_key(&event.keystroke.key, side, width, bounds)
                    else {
                        return;
                    };
                    cx.stop_propagation();
                    if let Some(on_resize) = &on_resize {
                        on_resize(next, true, window, cx);
                    }
                }
            })
            // The width it was dragged to, saved now rather than on every frame of the drag: the
            // band follows the edge, so the pointer's let go over it, or just out of it.
            .on_mouse_up(MouseButton::Left, {
                let save = save_dragged(&dragging, &on_resize, width);
                move |_, window, cx| save(window, cx)
            })
            .on_mouse_up_out(MouseButton::Left, {
                let save = save_dragged(&dragging, &on_resize, width);
                move |_, window, cx| save(window, cx)
            })
            .on_click(move |event, window, cx| {
                if event.click_count() == 2
                    && let Some(on_reset) = &on_reset
                {
                    on_reset(window, cx);
                }
            })
    }
}

/// Saves `width`, the width last rendered, if the band was being dragged: once it's let go of.
fn save_dragged(
    dragging: &Entity<bool>,
    on_resize: &Option<OnResize>,
    width: f32,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let dragging = dragging.clone();
    let on_resize = on_resize.clone();
    move |window, cx| {
        if !dragging.read(cx) {
            return;
        }
        dragging.update(cx, |dragging, _| *dragging = false);
        if let Some(on_resize) = &on_resize {
            on_resize(width, true, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Context, IntoElement, TestAppContext, div, point, px};

    use super::*;
    use crate::test_support;
    use gitto_app_data::services::LocalStorage;
    use gitto_app_data::state::panel_width::PanelWidth;

    const BOUNDS: PanelBounds = PanelBounds::new(220., 160., 480.);

    /// A panel on the left of its handle (TS's `renderHandle`), its width kept in `storage`.
    struct Host {
        panel: PanelWidth,
        side: HandleSide,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity().downgrade();
            let reset = this.clone();
            let side = self.side;
            div()
                .size_full()
                .flex()
                .on_drag_move(
                    cx.listener(move |host, event: &DragMoveEvent<PanelDrag>, _, cx| {
                        if let Some(width) = dragged_width(event, "test", side, cx) {
                            host.panel.set_width(width, false);
                            cx.notify();
                        }
                    }),
                )
                .child(div().flex_none().w(px(self.panel.width())).h_full())
                .child(
                    div().relative().w_0().h_full().child(
                        ResizeHandle::new("test", side, "Resize panel", self.panel.width(), BOUNDS)
                            .on_resize(move |width, save, _, cx| {
                                let _ = this.update(cx, |host, cx| {
                                    host.panel.set_width(width, save);
                                    cx.notify();
                                });
                            })
                            .on_reset(move |_, cx| {
                                let _ = reset.update(cx, |host, cx| {
                                    host.panel.reset();
                                    cx.notify();
                                });
                            }),
                    ),
                )
        }
    }

    fn render_handle(
        side: HandleSide,
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Rc<LocalStorage>,
        AnyWindowHandle,
        Entity<Host>,
    ) {
        let dir = test_support::init(cx);
        let storage = Rc::new(LocalStorage::in_memory());
        let panel = PanelWidth::new("test", BOUNDS, Rc::clone(&storage));
        let (window, host) = test_support::open(cx, |_, _| Host { panel, side });
        (dir, storage, window, host)
    }

    fn width(host: &Entity<Host>, cx: &mut TestAppContext) -> f32 {
        host.read_with(cx, |host, _| host.panel.width())
    }

    fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| window.press(key, cx))
            .unwrap();
    }

    #[gpui_kit::test]
    fn nudges_a_panel_on_its_left_with_the_arrow_keys_ui(cx: &mut TestAppContext) {
        let (_dir, _, window, host) = render_handle(HandleSide::Start, cx);
        cx.update_window(window, |_, window, cx| window.click("resize-test", cx))
            .unwrap();

        press(window, "right", cx);
        assert_eq!(width(&host, cx), 236.);
        press(window, "left", cx);
        press(window, "left", cx);
        assert_eq!(width(&host, cx), 204.);
        press(window, "end", cx);
        assert_eq!(width(&host, cx), 480.);
        press(window, "home", cx);
        assert_eq!(width(&host, cx), 160.);
    }

    #[gpui_kit::test]
    fn grows_a_panel_on_its_right_with_the_left_arrow_key_ui(cx: &mut TestAppContext) {
        let (_dir, _, window, host) = render_handle(HandleSide::End, cx);
        cx.update_window(window, |_, window, cx| window.click("resize-test", cx))
            .unwrap();
        press(window, "left", cx);
        assert_eq!(width(&host, cx), 236.);
    }

    #[gpui_kit::test]
    fn saves_the_width_its_dragged_to_once_its_let_go_of(cx: &mut TestAppContext) {
        let (_dir, storage, window, host) = render_handle(HandleSide::Start, cx);
        cx.update_window(window, |_, window, cx| {
            let start = window.find("resize-test").bounds().center();
            window.drag(start, point(px(320.), start.y), cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(width(&host, cx), 320.);
        assert_eq!(
            storage.get_item("gitto-panel-width:test").as_deref(),
            Some("320")
        );
    }

    #[gpui_kit::test]
    fn resets_the_width_on_double_click(cx: &mut TestAppContext) {
        let (_dir, _, window, host) = render_handle(HandleSide::Start, cx);
        host.update(cx, |host, cx| {
            host.panel.set_width(400., true);
            cx.notify();
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.double_click("resize-test", cx);
        })
        .unwrap();
        assert_eq!(width(&host, cx), 220.);
    }

    #[test]
    fn nudges_a_panel_on_its_left_with_the_arrow_keys() {
        let side = HandleSide::Start;
        assert_eq!(width_for_key("right", side, 220., BOUNDS), Some(236.));
        assert_eq!(width_for_key("left", side, 236., BOUNDS), Some(220.));
        assert_eq!(width_for_key("end", side, 220., BOUNDS), Some(480.));
        assert_eq!(width_for_key("home", side, 220., BOUNDS), Some(160.));
        assert_eq!(width_for_key("a", side, 220., BOUNDS), None);
    }

    #[test]
    fn grows_a_panel_on_its_right_with_the_left_arrow_key() {
        assert_eq!(
            width_for_key("left", HandleSide::End, 220., BOUNDS),
            Some(236.)
        );
    }
}
