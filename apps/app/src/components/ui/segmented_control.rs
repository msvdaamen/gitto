//! A choice of a few options, side by side. Ported from `components/ui/segmented-control.tsx`.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, SharedString, Window};

type OnChange<T> = Box<dyn Fn(T, &mut Window, &mut App) + 'static>;

/// A choice of a few options, side by side, the chosen one raised: GPUI Kit's segmented
/// `TabBar`, which keeps the silhouette the Design Guides ask for. Controlled: `on_change` asks
/// for the option picked, and the owner sets `value`. The options are the owner's values (an
/// enum's, say), each with its label.
#[derive(IntoElement)]
pub struct SegmentedControl<T: Copy + PartialEq + 'static> {
    id: ElementId,
    value: T,
    options: Vec<(T, SharedString)>,
    on_change: Option<OnChange<T>>,
}

impl<T: Copy + PartialEq + 'static> SegmentedControl<T> {
    pub fn new(
        id: impl Into<ElementId>,
        value: T,
        options: impl IntoIterator<Item = (T, impl Into<SharedString>)>,
    ) -> Self {
        SegmentedControl {
            id: id.into(),
            value,
            options: options
                .into_iter()
                .map(|(option, label)| (option, label.into()))
                .collect(),
            on_change: None,
        }
    }

    pub fn on_change(mut self, handler: impl Fn(T, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl<T: Copy + PartialEq + 'static> RenderOnce for SegmentedControl<T> {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let selected = self
            .options
            .iter()
            .position(|(option, _)| *option == self.value)
            .unwrap_or(0);
        let options: Vec<T> = self.options.iter().map(|(option, _)| *option).collect();
        TabBar::new(self.id)
            .segmented()
            .small()
            .selected_index(selected)
            .children(
                self.options
                    .into_iter()
                    .map(|(_, label)| Tab::new().label(label)),
            )
            .when_some(self.on_change, |bar, on_change| {
                bar.on_click(move |ix, window, cx| {
                    if let Some(option) = options.get(*ix) {
                        on_change(*option, window, cx);
                    }
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Context, TestAppContext};

    use super::*;
    use crate::test_support;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Mode {
        Branches,
        Files,
    }

    struct Host {
        picked: Rc<Cell<Option<Mode>>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let picked = Rc::clone(&self.picked);
            SegmentedControl::new(
                "mode",
                Mode::Branches,
                [(Mode::Branches, "Branches"), (Mode::Files, "Files")],
            )
            .on_change(move |mode, _, _| picked.set(Some(mode)))
        }
    }

    #[gpui_kit::test]
    fn says_which_option_was_picked(cx: &mut TestAppContext) {
        let _dir = test_support::init(cx, |services| services);
        let picked = Rc::new(Cell::new(None));
        let (window, _) = test_support::open(cx, {
            let picked = Rc::clone(&picked);
            move |_, _| Host { picked }
        });
        cx.update_window(window, |_, window, cx| {
            window.click(ElementId::from(1usize), cx)
        })
        .unwrap();
        assert_eq!(picked.get(), Some(Mode::Files));
    }
}
