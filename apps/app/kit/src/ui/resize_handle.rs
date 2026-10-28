//! A draggable line on the edge of a panel that resizes it. Ported from
//! `components/ui/resize-handle.tsx`.
//!
//! The band is GPUI Kit's `resize_handle`: its hit area, cursor, hover and drag. What the drag
//! resizes is its owner's: the element the panel's edge is in (TS's handle's parent) follows the
//! drag with `.on_drag_move::<PanelDrag<P>>`, where `P` names the owner's panels, and asks
//! [`dragged_width`] for the width at the pointer, setting it without saving; the handle saves
//! the width it's let go at (its `on_resize`, with `save`):
//!
//! ```ignore
//! h_flex()
//!     .on_drag_move(cx.listener(|this, event: &DragMoveEvent<PanelDrag<Panel>>, _, cx| {
//!         let panel = event.drag(cx).panel;
//!         this.width_of(panel).set_width(dragged_width(event, panel.side()), false);
//!     }))
//! ```
//!
//! With focus, its keys are actions ([`GrowPanel`], [`ShrinkPanel`], [`PanelMin`],
//! [`PanelMax`]), bound by [`bind_keys`] in the `ResizeHandle` key context.

use std::ops::RangeInclusive;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Axis, DragMoveEvent, ElementId, Empty, Entity, FocusHandle, KeyBinding, KeyContext,
    MouseButton, Role, SharedString, Window, actions,
};

/// How far an arrow key moves the edge, in pixels.
pub const KEYBOARD_STEP: f32 = 16.;

actions!(
    resize_handle,
    [
        /// Widens the focused handle's panel by a step.
        GrowPanel,
        /// Narrows the focused handle's panel by a step.
        ShrinkPanel,
        /// Takes the focused handle's panel to its narrowest.
        PanelMin,
        /// Takes the focused handle's panel to its widest.
        PanelMax
    ]
);

/// The key context of a focused handle, with its `side` (`start` or `end`).
pub const KEY_CONTEXT: &str = "ResizeHandle";

/// Binds the handles' keys: the arrow keys move the edge (the one pointing into the panel
/// shrinks it), Home and End take it to its narrowest and widest. Call once, as the app starts.
pub fn bind_keys(cx: &mut App) {
    let start = Some("ResizeHandle && side == start");
    let end = Some("ResizeHandle && side == end");
    cx.bind_keys([
        KeyBinding::new("right", GrowPanel, start),
        KeyBinding::new("left", ShrinkPanel, start),
        KeyBinding::new("left", GrowPanel, end),
        KeyBinding::new("right", ShrinkPanel, end),
        KeyBinding::new("home", PanelMin, Some(KEY_CONTEXT)),
        KeyBinding::new("end", PanelMax, Some(KEY_CONTEXT)),
    ]);
}

/// Which side of the edge its panel is on: `Start` for a panel on its left (the sidebar), `End`
/// for one on its right (the details).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleSide {
    Start,
    End,
}

impl HandleSide {
    /// As the handle's key context has it.
    fn as_str(self) -> &'static str {
        match self {
            HandleSide::Start => "start",
            HandleSide::End => "end",
        }
    }
}

/// What a panel's handle drags: the panel, as its owner names its panels (an enum, say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelDrag<P> {
    pub panel: P,
}

/// Nothing follows the pointer: the panel's edge does.
impl<P: 'static> Render for PanelDrag<P> {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The width the dragged panel takes with its edge at the pointer, measured in the element
/// listening (the edge's parent), for a panel on `side` of its edge.
pub fn dragged_width<P: 'static>(event: &DragMoveEvent<PanelDrag<P>>, side: HandleSide) -> f32 {
    let x = event.event.position.x;
    let width = match side {
        HandleSide::Start => x - event.bounds.left(),
        HandleSide::End => event.bounds.right() - x,
    };
    f32::from(width)
}

type OnResize = Rc<dyn Fn(f32, bool, &mut Window, &mut App) + 'static>;
type OnReset = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// A draggable line on the edge of a panel that resizes it (see the module's doc). Double-click
/// resets the width; with focus, the arrow keys nudge it, Home and End take it to the narrowest
/// and widest the panel can be made, `range`.
/// `on_resize` is given the width to resize to, and whether to save it: always, from the
/// keyboard, and once a drag is let go of; not while it's dragged.
#[derive(IntoElement)]
pub struct ResizeHandle<P: Copy + 'static> {
    id: ElementId,
    panel: P,
    side: HandleSide,
    label: SharedString,
    width: f32,
    range: RangeInclusive<f32>,
    on_resize: Option<OnResize>,
    on_reset: Option<OnReset>,
}

impl<P: Copy + 'static> ResizeHandle<P> {
    /// The handle `id` of the panel `panel`, on `side` of its edge.
    pub fn new(
        id: impl Into<ElementId>,
        panel: P,
        side: HandleSide,
        label: impl Into<SharedString>,
        width: f32,
        range: RangeInclusive<f32>,
    ) -> Self {
        ResizeHandle {
            id: id.into(),
            panel,
            side,
            label: label.into(),
            width,
            range,
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

impl<P: Copy + 'static> RenderOnce for ResizeHandle<P> {
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
        let range = self.range;
        let on_resize = self.on_resize;
        let on_reset = self.on_reset;
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        key_context.set("side", side.as_str());
        // Resizes to `to` from the keyboard, saved.
        let resize_to = |to: f32| {
            let on_resize = on_resize.clone();
            move |window: &mut Window, cx: &mut App| {
                if let Some(on_resize) = &on_resize {
                    on_resize(to, true, window, cx);
                }
            }
        };
        let grow = resize_to(width + KEYBOARD_STEP);
        let shrink = resize_to(width - KEYBOARD_STEP);
        let min = resize_to(*range.start());
        let max = resize_to(*range.end());
        gpui_kit::component::resize_handle::<PanelDrag<P>, PanelDrag<P>>(self.id, Axis::Horizontal)
            .on_drag(PanelDrag { panel }, {
                let dragging = dragging.clone();
                move |drag, _, _, cx| {
                    dragging.update(cx, |dragging, _| *dragging = true);
                    cx.new(|_| *drag)
                }
            })
            .track_focus(&focus)
            .key_context(key_context)
            .role(Role::Splitter)
            .aria_label(self.label)
            .aria_numeric_value(f64::from(width))
            .on_action(move |_: &GrowPanel, window, cx| grow(window, cx))
            .on_action(move |_: &ShrinkPanel, window, cx| shrink(window, cx))
            .on_action(move |_: &PanelMin, window, cx| min(window, cx))
            .on_action(move |_: &PanelMax, window, cx| max(window, cx))
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
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, Context, IntoElement, TestAppContext, div, point, px};

    use super::*;
    use crate::test_support;

    const INITIAL: f32 = 220.;
    const RANGE: RangeInclusive<f32> = 160.0..=480.;

    /// A panel's width as its owner keeps it: within its range, and the width last saved.
    struct Panel {
        width: f32,
        saved: Option<f32>,
    }

    impl Panel {
        fn width(&self) -> f32 {
            self.width
        }

        fn set_width(&mut self, width: f32, save: bool) {
            self.width = width.clamp(*RANGE.start(), *RANGE.end()).round();
            if save {
                self.saved = Some(self.width);
            }
        }

        fn reset(&mut self) {
            self.set_width(INITIAL, true);
        }
    }

    /// A panel on the left of its handle (TS's `renderHandle`).
    struct Host {
        panel: Panel,
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
                .on_drag_move(cx.listener(
                    move |host, event: &DragMoveEvent<PanelDrag<()>>, _, cx| {
                        host.panel.set_width(dragged_width(event, side), false);
                        cx.notify();
                    },
                ))
                .child(div().flex_none().w(px(self.panel.width())).h_full())
                .child(
                    div().relative().w_0().h_full().child(
                        ResizeHandle::new(
                            "resize-test",
                            (),
                            side,
                            "Resize panel",
                            self.panel.width(),
                            RANGE,
                        )
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

    fn render_handle(side: HandleSide, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Host>) {
        test_support::init(cx);
        cx.update(bind_keys);
        let panel = Panel {
            width: INITIAL,
            saved: None,
        };
        test_support::open(cx, |_, _| Host { panel, side })
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
        let (window, host) = render_handle(HandleSide::Start, cx);
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
        let (window, host) = render_handle(HandleSide::End, cx);
        cx.update_window(window, |_, window, cx| window.click("resize-test", cx))
            .unwrap();
        press(window, "left", cx);
        assert_eq!(width(&host, cx), 236.);
    }

    #[gpui_kit::test]
    fn saves_the_width_its_dragged_to_once_its_let_go_of(cx: &mut TestAppContext) {
        let (window, host) = render_handle(HandleSide::Start, cx);
        cx.update_window(window, |_, window, cx| {
            let start = window.find("resize-test").bounds().center();
            window.drag(start, point(px(320.), start.y), cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(width(&host, cx), 320.);
        assert_eq!(host.read_with(cx, |host, _| host.panel.saved), Some(320.));
    }

    #[gpui_kit::test]
    fn resets_the_width_on_double_click(cx: &mut TestAppContext) {
        let (window, host) = render_handle(HandleSide::Start, cx);
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

    #[gpui_kit::test]
    fn shrinks_a_panel_on_its_right_with_the_right_arrow_key_ui(cx: &mut TestAppContext) {
        let (window, host) = render_handle(HandleSide::End, cx);
        cx.update_window(window, |_, window, cx| window.click("resize-test", cx))
            .unwrap();
        press(window, "right", cx);
        assert_eq!(width(&host, cx), 204.);
        press(window, "a", cx);
        assert_eq!(width(&host, cx), 204.);
    }

    #[gpui_kit::test]
    fn resizes_with_its_actions_dispatched_to_it(cx: &mut TestAppContext) {
        let (window, host) = render_handle(HandleSide::Start, cx);
        cx.update_window(window, |_, window, cx| {
            window.click("resize-test", cx);
            window.dispatch_action(Box::new(PanelMax), cx);
        })
        .unwrap();
        assert_eq!(width(&host, cx), 480.);
        assert_eq!(host.read_with(cx, |host, _| host.panel.saved), Some(480.));
    }
}
