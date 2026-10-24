//! Lists that only render the rows in view. Ported from `components/ui/virtual-list.tsx`.
//!
//! GPUI's `uniform_list` is the virtualizer: it measures one row, and only renders the ones in
//! view, by position, so a list of thousands costs as much as a screenful, and items that are
//! replaced (by a reload, or a folder collapsing above them) are rendered in place. [`VirtualList`]
//! gives every row the same height, as TS's `rowHeight` did, and scrolls in its own container.
//!
//! TS's `VirtualRows` (rows sharing a scroll container with content around them, keeping the
//! focused row rendered) has no counterpart: keyboard focus in a GPUI list is the owner's selected
//! index, not an element's, so nothing's lost when a row scrolls out; content around a list goes
//! in its own region. [`is_near_end`] says when the rows in view come near the end, to load more.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, Rems, UniformListScrollHandle, Window, div, uniform_list,
};

type RenderRows = Rc<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

/// A scrolling list that only renders the rows in view (see the module's doc). Its rows are
/// rendered by `render_rows`, given the range in view: keep it side-effect-free, as it runs on
/// every frame the list is drawn.
#[derive(IntoElement)]
pub struct VirtualList {
    id: ElementId,
    count: usize,
    row_height: Rems,
    render_rows: RenderRows,
    scroll: Option<UniformListScrollHandle>,
}

impl VirtualList {
    pub fn new(
        id: impl Into<ElementId>,
        count: usize,
        row_height: Rems,
        render_rows: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    ) -> Self {
        VirtualList {
            id: id.into(),
            count,
            row_height,
            render_rows: Rc::new(render_rows),
            scroll: None,
        }
    }

    /// Follows (and lets the owner move) its scroll position, e.g. to scroll a selected row into
    /// view, or back to the top for another repository.
    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }
}

impl RenderOnce for VirtualList {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let render_rows = self.render_rows;
        let row_height = self.row_height;
        uniform_list(self.id, self.count, move |range, window, cx| {
            render_rows(range, window, cx)
                .into_iter()
                .map(|row| div().h(row_height).overflow_hidden().child(row))
                .collect::<Vec<_>>()
        })
        .when_some(self.scroll, |list, scroll| list.track_scroll(&scroll))
        .size_full()
    }
}

/// Whether the rows in view, `range`, have come within `within` rows of the last of `count`
/// (TS's `onNearEndChange`), e.g. to load more as the end comes near.
pub fn is_near_end(range: &Range<usize>, count: usize, within: usize) -> bool {
    count > 0 && range.end + within >= count
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Context, Render, TestAppContext, WindowBounds, WindowOptions, px,
        rems, size,
    };

    use super::*;

    /// A list of 100 rows, recording which were rendered last.
    struct Rows {
        items: Rc<RefCell<Vec<String>>>,
        rendered: Rc<RefCell<Range<usize>>>,
    }

    impl Render for Rows {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let items = Rc::clone(&self.items);
            let rendered = Rc::clone(&self.rendered);
            let count = items.borrow().len();
            div().size_full().child(VirtualList::new(
                "rows",
                count,
                rems(1.),
                move |range, _, _| {
                    *rendered.borrow_mut() = range.clone();
                    range
                        .map(|ix| div().child(items.borrow()[ix].clone()).into_any_element())
                        .collect()
                },
            ))
        }
    }

    fn open(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<Rows>) {
        cx.update(gpui_kit::init);
        let items = Rc::new(RefCell::new(
            (0..100).map(|ix| format!("Row {ix}")).collect(),
        ));
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(200.), px(160.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|_| Rows {
                        items,
                        rendered: Rc::new(RefCell::new(0..0)),
                    })
                },
            )
            .expect("open a window")
        })
    }

    #[gpui_kit::test]
    fn only_renders_the_rows_in_view(cx: &mut TestAppContext) {
        let (window, rows) = open(cx);
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        let rendered = rows.read_with(cx, |rows, _| rows.rendered.borrow().clone());
        assert_eq!(rendered.start, 0);
        assert!(rendered.end > 5 && rendered.end < 30, "{rendered:?}");
    }

    #[gpui_kit::test]
    fn shows_the_items_that_replace_the_ones_in_view_in_the_same_rows(cx: &mut TestAppContext) {
        let (window, rows) = open(cx);
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        rows.update(cx, |rows, cx| {
            *rows.items.borrow_mut() = vec!["Other 0".into(), "Other 1".into(), "Other 2".into()];
            cx.notify();
        });
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        let rendered = rows.read_with(cx, |rows, _| rows.rendered.borrow().clone());
        assert_eq!(rendered, 0..3);
    }

    #[test]
    fn says_when_the_rows_in_view_come_near_the_end() {
        assert!(!is_near_end(&(0..20), 100, 20));
        assert!(is_near_end(&(60..80), 100, 20));
        assert!(!is_near_end(&(0..0), 0, 20));
    }
}
