//! The header's tabs: home, then a tab per repository. Ported from `components/tabs.tsx`.

use std::collections::HashMap;

use gitto_git::input::RepositoryId;
use gitto_repository::Repository;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, FocusHandle, FontWeight, KeyDownEvent, SharedString, Subscription,
    TestSupportExt as _, Window, div, rems,
};

use super::remove_repository_dialog::open_remove_repository_dialog;
use gitto_app_data::data::repositories::{
    OpenRepository, Opened, remove_repository, reorder_repositories,
};
use gitto_app_data::data::{AppChanged, AppData, AppPart, DataError};
use gitto_app_data::state::router::{Route, Router};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::{FailurePopover, GittoIcon, Icon, IconName};

/// Below these window widths the home tab shows its icon alone (`md`), and the repositories' tabs
/// can be narrower (`sm`).
const MEDIUM: f32 = 768.;
const SMALL: f32 = 640.;

/// The ids in `list` that are in `other` too, in `list`'s order.
fn common<'a>(list: &'a [String], other: &[String]) -> Vec<&'a String> {
    list.iter().filter(|id| other.contains(id)).collect()
}

/// Whether the ids in both `a` and `b` are in the same order in each.
pub fn same_order(a: &[String], b: &[String]) -> bool {
    common(a, b) == common(b, a)
}

/// `order` with `id` moved to where `target` is.
pub fn moved_to(order: &[String], id: &str, target: &str) -> Vec<String> {
    let mut order: Vec<String> = order.to_vec();
    let Some(from) = order.iter().position(|other| other == id) else {
        return order;
    };
    let moving = order.remove(from);
    let to = order
        .iter()
        .position(|other| other == target)
        .map_or(order.len(), |to| if to >= from { to + 1 } else { to });
    order.insert(to.min(order.len()), moving);
    order
}

/// What a dragged tab carries: its repository.
#[derive(Debug, Clone)]
pub struct TabDrag {
    id: String,
    name: SharedString,
}

/// The tab under the pointer while it's dragged.
impl Render for TabDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = *cx.palette();
        h_flex()
            .h_8()
            .px_3()
            .gap_2()
            .rounded_md()
            .bg(palette.bg)
            .border_1()
            .border_color(palette.border)
            .shadow_lg()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.text)
            .child(
                Icon::new(IconName::GitBranch)
                    .size_3p5()
                    .text_color(palette.blue),
            )
            .child(self.name.clone())
    }
}

/// A tab per repository, after the home tab. The page on show decides which one is selected:
/// none, on a page of its own like the settings. The repositories' tabs are dragged, or moved with
/// Alt+Shift and the arrow keys, into another order; their close button removes them, asking
/// first.
pub struct Tabs {
    data: Entity<AppData>,
    open_repository: Entity<OpenRepository>,
    /// The tabs' order once one's moved, until that's saved and read back.
    moved_order: Option<Vec<String>>,
    /// The tab last moved, which says why if the new order couldn't be saved.
    moved: Option<String>,
    move_error: Option<DataError>,
    /// The repository last tried to be removed, and why that failed.
    removing: Option<String>,
    remove_error: Option<DataError>,
    focus: HashMap<String, FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl Tabs {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let data = AppData::global(cx);
        data.update(cx, |data, cx| data.show(AppPart::Repositories, cx));
        let open_repository = OpenRepository::new(cx);
        let subscriptions = vec![
            cx.observe(&Router::global(cx), |_, _, cx| cx.notify()),
            cx.observe(&open_repository, |_, _, cx| cx.notify()),
            cx.subscribe(&open_repository, |_, _, Opened(id), cx| {
                Router::navigate(Route::Repository(id.clone()), cx);
            }),
            cx.subscribe(&data, |tabs, _, AppChanged(part), cx| {
                if *part == AppPart::Repositories {
                    tabs.forget_saved_order(cx);
                    cx.notify();
                }
            }),
        ];
        Tabs {
            data,
            open_repository,
            moved_order: None,
            moved: None,
            move_error: None,
            removing: None,
            remove_error: None,
            focus: HashMap::new(),
            _subscriptions: subscriptions,
        }
    }

    fn repositories<'a>(&self, cx: &'a App) -> &'a [Repository] {
        self.data
            .read(cx)
            .repositories()
            .data()
            .map_or(&[], Vec::as_slice)
    }

    fn saved_order(&self, cx: &App) -> Vec<String> {
        self.repositories(cx)
            .iter()
            .map(|repository| repository.id.clone())
            .collect()
    }

    /// The repositories' ids in the order their tabs are shown: ids rather than the repositories,
    /// so the tabs stay as they are when the list is read again.
    pub fn ordered(&self, cx: &App) -> Vec<String> {
        self.moved_order
            .clone()
            .unwrap_or_else(|| self.saved_order(cx))
    }

    /// The tabs show the saved order again once the order they were moved into is read back.
    fn forget_saved_order(&mut self, cx: &mut Context<Self>) {
        if let Some(order) = &self.moved_order
            && same_order(order, &self.saved_order(cx))
        {
            self.moved_order = None;
            cx.notify();
        }
    }

    /// Shows the tabs in `order` (with `id` moved) until it's saved, unless it's the order they
    /// had; puts them back, saying why, if it can't be saved.
    fn reorder(&mut self, id: &str, order: Vec<String>, cx: &mut Context<Self>) {
        let unchanged = same_order(&order, &self.saved_order(cx));
        self.moved_order = Some(order.clone());
        cx.notify();
        if unchanged {
            return;
        }
        self.moved = Some(id.to_owned());
        self.move_error = None;
        let saving = reorder_repositories(order, cx);
        cx.spawn(async move |this, cx| {
            let result = saving.await;
            let _ = this.update(cx, |tabs, cx| {
                if let Err(error) = result {
                    tabs.moved_order = None;
                    tabs.move_error = Some(error);
                }
                tabs.forget_saved_order(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Moves the tab `id` one place left (`-1`) or right (`1`); it keeps focus.
    pub fn move_tab(&mut self, id: &str, by: isize, cx: &mut Context<Self>) {
        let mut order = self.ordered(cx);
        let Some(from) = order.iter().position(|other| other == id) else {
            return;
        };
        let to = from as isize + by;
        if to < 0 || to as usize >= order.len() {
            return;
        }
        let moving = order.remove(from);
        order.insert(to as usize, moving);
        self.reorder(id, order, cx);
    }

    fn drop_on(&mut self, dragged: &str, target: &str, cx: &mut Context<Self>) {
        if dragged == target {
            return;
        }
        let order = moved_to(&self.ordered(cx), dragged, target);
        self.reorder(dragged, order, cx);
    }

    fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        self.removing = Some(id.clone());
        self.remove_error = None;
        let removing = remove_repository(id.clone(), cx);
        cx.spawn(async move |this, cx| {
            let result = removing.await;
            let _ = this.update(cx, |tabs, cx| {
                match result {
                    Ok(()) => {
                        if Router::current(cx)
                            .repository_id()
                            .map(|shown| shown.as_str())
                            == Some(id.as_str())
                        {
                            Router::navigate(Route::Home, cx);
                        }
                    }
                    Err(error) => tabs.remove_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn dismiss_errors(&mut self, cx: &mut Context<Self>) {
        self.move_error = None;
        self.remove_error = None;
        cx.notify();
    }

    /// The home tab: Gitto's icon and name, or below `MEDIUM` the icon alone.
    fn render_home_tab(
        &self,
        selected: bool,
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let palette = *cx.palette();
        tab_frame("home-tab", selected, cx)
            .w(if narrow {
                rems(53. / 15.)
            } else {
                rems(116. / 15.)
            })
            .flex_none()
            .aria_label("Home")
            .when(narrow, |this| this.justify_center())
            .when(!narrow, |this| this.px_3p5())
            .gap_2()
            .on_click(|_, _, cx| Router::navigate(Route::Home, cx))
            .child(GittoIcon::new().w_6().h_5())
            .when(!narrow, |this| {
                this.child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(palette.text)
                        .child("gitto"),
                )
            })
            .test_support()
    }

    fn render_repository_tab(
        &mut self,
        repository: &Repository,
        selected: bool,
        small: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let palette = *cx.palette();
        let id = repository.id.clone();
        let focus = self
            .focus
            .entry(id.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let error = if self.moved.as_deref() == Some(id.as_str()) && self.move_error.is_some() {
            Some(("Move", self.move_error.clone()))
        } else if self.removing.as_deref() == Some(id.as_str()) {
            Some(("Remove", self.remove_error.clone()))
        } else {
            None
        };
        let (action, error) = error.unwrap_or(("Remove", None));
        let name: SharedString = repository.name.clone().into();
        let group = SharedString::from(format!("tab-group-{id}"));
        tab_frame(SharedString::from(format!("tab-{id}")), selected, cx)
            .min_w(if small { rems(7.) } else { rems(8.) })
            .max_w(rems(12.))
            .border_r_1()
            .border_color(palette.border_soft)
            .track_focus(&focus)
            .aria_label(name.clone())
            .aria_keyshortcuts("Alt+Shift+ArrowLeft Alt+Shift+ArrowRight")
            .on_click({
                let id = id.clone();
                move |_, _, cx| {
                    if let Ok(id) = RepositoryId::parse(id.clone()) {
                        Router::navigate(Route::Repository(id), cx);
                    }
                }
            })
            .on_key_down(cx.listener({
                let id = id.clone();
                move |tabs, event: &KeyDownEvent, _, cx| {
                    let modifiers = event.keystroke.modifiers;
                    if !modifiers.alt || !modifiers.shift {
                        return;
                    }
                    let by = match event.keystroke.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        _ => return,
                    };
                    cx.stop_propagation();
                    tabs.move_tab(&id, by, cx);
                }
            }))
            .on_drag(
                TabDrag {
                    id: id.clone(),
                    name: name.clone(),
                },
                |drag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .drag_over::<TabDrag>(|style, _, _, cx| style.bg(cx.palette().panel_hover))
            .on_drop(cx.listener({
                let id = id.clone();
                move |tabs, drag: &TabDrag, _, cx| tabs.drop_on(&drag.id, &id, cx)
            }))
            .group(group.clone())
            .pl_2p5()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size_5()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .bg(palette.blue_soft)
                    .text_color(palette.blue)
                    .child(Icon::new(IconName::GitBranch).size_3p5()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if selected {
                        palette.text
                    } else {
                        palette.muted
                    })
                    .child(name.clone()),
            )
            .child(
                // Shown on the selected tab, and on another while it's hovered.
                div()
                    .mr_2()
                    .ml_1()
                    .when(!selected && error.is_none(), |this| this.opacity(0.))
                    .group_hover(group, |style| style.opacity(1.))
                    .child(
                        FailurePopover::new(
                            SharedString::from(format!("tab-failure-{id}")),
                            format!("{action} {name}"),
                            error.map(|error| error.to_string().into()),
                            Button::new(SharedString::from(format!("remove-{id}")))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::X))
                                .tooltip(format!("Remove {name}"))
                                .accessibility_label(format!("Remove {name}"))
                                .on_click(cx.listener({
                                    let repository = repository.clone();
                                    move |_, _, window, cx| {
                                        let tabs = cx.entity();
                                        let id = repository.id.clone();
                                        open_remove_repository_dialog(
                                            &repository,
                                            move |_, cx| {
                                                let id = id.clone();
                                                tabs.update(cx, |tabs, cx| tabs.remove(id, cx));
                                            },
                                            window,
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .on_dismiss({
                            let tabs = cx.entity();
                            move |_, cx| tabs.update(cx, |tabs, cx| tabs.dismiss_errors(cx))
                        }),
                    ),
            )
            .test_support()
    }
}

/// A tab's box: on the window's background while selected, with the primary underline.
fn tab_frame(
    id: impl Into<gpui_kit::ElementId>,
    selected: bool,
    cx: &App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let palette = *cx.palette();
    h_flex()
        .id(id)
        .h_full()
        .relative()
        .text_sm()
        .when(selected, |this| this.bg(palette.bg))
        .when(!selected, |this| {
            this.hover(|style| style.bg(palette.panel_hover))
        })
        .when(selected, |this| {
            this.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h_0p5()
                    .bg(palette.primary),
            )
        })
}

impl Render for Tabs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let route = Router::current(cx);
        let width = f32::from(window.viewport_size().width);
        let selected = |id: &str| {
            route
                .repository_id()
                .is_some_and(|shown| shown.as_str() == id)
        };
        let repositories = self.repositories(cx).to_vec();
        let mut tabs = Vec::new();
        for id in self.ordered(cx) {
            if let Some(repository) = repositories.iter().find(|repository| repository.id == id) {
                tabs.push(
                    self.render_repository_tab(repository, selected(&id), width < SMALL, cx)
                        .into_any_element(),
                );
            }
        }
        let open_error = self.open_repository.read(cx).error().cloned();
        let open_repository = self.open_repository.clone();
        h_flex()
            .h_full()
            .items_stretch()
            .min_w_0()
            .child(self.render_home_tab(route == Route::Home, width < MEDIUM, cx))
            .children(tabs)
            .child(
                div().ml_1().flex().items_center().child(
                    FailurePopover::new(
                        "open-repository-failure",
                        "Open repository",
                        open_error.map(|error| error.to_string().into()),
                        Button::new("open-repository")
                            .ghost()
                            .small()
                            .icon(Icon::new(IconName::Plus))
                            .tooltip("Open new repository")
                            .accessibility_label("Open new repository")
                            .on_click({
                                let open_repository = open_repository.clone();
                                move |_, _, cx| {
                                    open_repository.update(cx, |open, cx| open.open(cx));
                                }
                            }),
                    )
                    .on_dismiss(move |_, cx| {
                        open_repository.update(cx, |open, cx| open.dismiss(cx));
                    }),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn tells_orders_apart_by_the_ids_both_have() {
        assert!(same_order(&ids(&["a", "b", "c"]), &ids(&["a", "c"])));
        assert!(!same_order(&ids(&["b", "a"]), &ids(&["a", "b", "c"])));
    }

    #[test]
    fn moves_a_dropped_tab_to_where_it_was_dropped() {
        let order = ids(&["a", "b", "c"]);
        assert_eq!(moved_to(&order, "a", "b"), ids(&["b", "a", "c"]));
        assert_eq!(moved_to(&order, "c", "a"), ids(&["c", "a", "b"]));
        assert_eq!(moved_to(&order, "a", "c"), ids(&["b", "c", "a"]));
    }
}

#[cfg(test)]
mod ui_tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    use futures::FutureExt as _;
    use futures::future::BoxFuture;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, IntoElement, Render, TestAppContext};

    use super::*;
    use crate::test_support;
    use gitto_app_data::data::repositories::RepositorySource;

    /// The repositories as saved, which `reorder` changes, unless it's told to fail (TS's mocked
    /// `rpc.repository`).
    #[derive(Default)]
    struct Saved {
        repositories: RefCell<Vec<Repository>>,
        reorders: RefCell<Vec<Vec<String>>>,
        fail: Cell<bool>,
    }

    impl RepositorySource for Saved {
        fn list(&self) -> BoxFuture<'static, Result<Vec<Repository>, DataError>> {
            let repositories = self.repositories.borrow().clone();
            async move { Ok(repositories) }.boxed()
        }

        fn add(&self, _: PathBuf) -> BoxFuture<'static, Result<Repository, DataError>> {
            async { Err(DataError::msg("Not in this test.")) }.boxed()
        }

        fn remove(&self, _: String) -> BoxFuture<'static, Result<(), DataError>> {
            async { Err(DataError::msg("Not in this test.")) }.boxed()
        }

        fn reorder(&self, ids: Vec<String>) -> BoxFuture<'static, Result<(), DataError>> {
            self.reorders.borrow_mut().push(ids.clone());
            if self.fail.get() {
                return async { Err(DataError::msg("The database is locked.")) }.boxed();
            }
            let mut repositories = self.repositories.borrow_mut();
            repositories.sort_by_key(|repository| {
                ids.iter()
                    .position(|id| *id == repository.id)
                    .unwrap_or(usize::MAX)
            });
            async { Ok(()) }.boxed()
        }
    }

    struct Host {
        tabs: Entity<Tabs>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().h_12().flex().child(self.tabs.clone())
        }
    }

    fn repository(name: &str) -> Repository {
        Repository {
            id: uuid::Uuid::now_v7().to_string(),
            name: format!("repo-{name}"),
            path: format!("/w/{name}"),
        }
    }

    /// `repo-a`, `repo-b` and `repo-c`'s tabs.
    fn render_tabs(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Rc<Saved>,
        Vec<String>,
        AnyWindowHandle,
        Entity<Host>,
    ) {
        let saved = Rc::new(Saved::default());
        *saved.repositories.borrow_mut() = ["a", "b", "c"].map(repository).to_vec();
        let ids = saved
            .repositories
            .borrow()
            .iter()
            .map(|repository| repository.id.clone())
            .collect();
        let dir = test_support::init(cx, |services| {
            services.with_repository_source(saved.clone())
        });
        let (window, host) = test_support::open(cx, |_, cx| Host {
            tabs: cx.new(Tabs::new),
        });
        cx.run_until_parked();
        (dir, saved, ids, window, host)
    }

    fn order(host: &Entity<Host>, cx: &mut TestAppContext) -> Vec<String> {
        host.read_with(cx, |host, cx| host.tabs.read(cx).ordered(cx))
    }

    fn move_right(id: &str, window: AnyWindowHandle, cx: &mut TestAppContext) {
        cx.update_window(window, |_, window, cx| {
            window.click(SharedString::from(format!("tab-{id}")), cx);
            window.press("alt-shift-right", cx);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn moves_a_tab_with_alt_shift_and_the_arrow_keys_keeping_it_focused(cx: &mut TestAppContext) {
        let (_dir, saved, ids, window, host) = render_tabs(cx);
        assert_eq!(order(&host, cx), ids);

        move_right(&ids[0], window, cx);
        let moved = vec![ids[1].clone(), ids[0].clone(), ids[2].clone()];
        assert_eq!(order(&host, cx), moved);
        cx.run_until_parked();

        assert_eq!(*saved.reorders.borrow(), std::slice::from_ref(&moved));
        assert!(host.read_with(cx, |host, cx| host.tabs.read(cx).moved_order.is_none()));
        assert_eq!(order(&host, cx), moved);
        let focused = cx
            .update_window(window, |_, window, cx| {
                host.read(cx).tabs.read(cx).focus[&ids[0]].is_focused(window)
            })
            .unwrap();
        assert!(focused);
    }

    #[gpui_kit::test]
    fn puts_the_tabs_back_and_says_why_when_the_order_cant_be_saved(cx: &mut TestAppContext) {
        let (_dir, saved, ids, window, host) = render_tabs(cx);
        saved.fail.set(true);

        move_right(&ids[0], window, cx);
        cx.run_until_parked();

        assert_eq!(order(&host, cx), ids);
        let error = host.read_with(cx, |host, cx| host.tabs.read(cx).move_error.clone());
        assert_eq!(error.unwrap().to_string(), "The database is locked.");
        let moved = host.read_with(cx, |host, cx| host.tabs.read(cx).moved.clone());
        assert_eq!(moved.as_deref(), Some(ids[0].as_str()));
    }
}
