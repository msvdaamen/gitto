//! A commit's branches and tags in the history. Ported from
//! `components/repository/history/ref-labels.tsx`.

use std::rc::Rc;

use gitto_ui::git::ref_labels::{RefLabel, describe_ref_label, ref_label_matches};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ElementId, FontWeight, Hsla, Role, SharedString, TestSupportExt as _, Window,
    anchored, deferred, div, px, rems,
};

use super::columns::{LABEL_ICON, LABEL_LINE, LABEL_TEXT, mixed, part, tinted};
use crate::components::repository::branch_menu::branch_menu;
use crate::components::ui::{Icon, IconName, menu_frame};
use crate::theme::{Palette, PaletteExt as _};

/// Switches to a branch, by its full ref name; or opens a worktree, by its path.
pub type OnRef = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// Told when the pointer enters (`true`) or leaves something.
pub type OnHover = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// What a row's labels do, and what they show of the rest of the history.
#[derive(Clone)]
pub struct LabelActions {
    /// Switches to a branch, by its full ref name, when its label is double-clicked.
    pub switch: OnRef,
    /// Opens the worktree a branch is checked out in, by its path, when its label is
    /// double-clicked: git won't switch to it here.
    pub open_worktree: OnRef,
    /// The full ref name of the branch whose menu is open, if any.
    pub menu_open_for: Option<SharedString>,
}

/// The labels in the order they're shown: while searching (`needle`, lowercase), the first one
/// the search matches comes first, so the row shows why it matched.
pub fn ordered_labels<'a>(labels: &'a [RefLabel], needle: &str) -> Vec<&'a RefLabel> {
    let mut ordered: Vec<&RefLabel> = labels.iter().collect();
    if !needle.is_empty()
        && let Some(found) = labels
            .iter()
            .position(|label| ref_label_matches(label, needle))
        && found > 0
    {
        let label = ordered.remove(found);
        ordered.insert(0, label);
    }
    ordered
}

/// What a label's tooltip says: what it is, and for a branch that isn't checked out, what
/// double-clicking it does.
pub fn label_title(label: &RefLabel) -> String {
    let described = describe_ref_label(label);
    match label {
        RefLabel::Branch(branch) if !branch.current => {
            if branch.worktree.is_some() {
                format!("{described}\nDouble-click to open that worktree")
            } else {
                format!("{described}\nDouble-click to switch to it")
            }
        }
        _ => described,
    }
}

/// What double-clicking `label` does: switch to the branch, by its full ref name, or open the
/// worktree it's checked out in, by its path; nothing for a tag, a detached HEAD or a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelTarget {
    Switch(String),
    OpenWorktree(String),
}

pub fn label_target(label: &RefLabel) -> Option<LabelTarget> {
    match label {
        RefLabel::Branch(branch) => Some(match &branch.worktree {
            Some(worktree) => LabelTarget::OpenWorktree(worktree.path.clone()),
            None => LabelTarget::Switch(branch.full_name.clone()),
        }),
        _ => None,
    }
}

/// The commit's branches and tags, tinted in its lane's colour like GitKraken: the first one,
/// with a count of the rest, which hovering it lists. A branch shows whether it's local (laptop),
/// on a remote (cloud) or both, and whether it's checked out in another worktree (folder), as does
/// a worktree checked out at the commit with no branch. While searching, a label the search
/// matches comes first, so the row shows why it matched. Double-clicking a branch switches to it,
/// or opens the worktree it's checked out in, as git won't switch to it here; and right-clicking
/// one opens its menu (see `branch_menu`), like in the sidebar.
///
/// Whether the rest are listed is the table's (see `MoreLabels`): it opens them after the count's
/// hovered a moment, and keeps them while a menu opened from them is.
#[derive(IntoElement)]
pub struct HistoryRefLabels {
    id: ElementId,
    labels: Vec<RefLabel>,
    needle: SharedString,
    color: Hsla,
    actions: LabelActions,
    more: Option<MoreLabels>,
}

/// The count of the labels a row has no room for, and the list of them it opens. The list
/// neither takes the focus from the history, nor closes as it's clicked: it's only hovered, and
/// its owner closes it once the pointer's been off it for a moment.
pub struct MoreLabels {
    /// Whether the list is open.
    pub open: bool,
    /// Told when the pointer enters or leaves the count or the list.
    pub on_hover: OnHover,
}

impl HistoryRefLabels {
    pub fn new(
        id: impl Into<ElementId>,
        labels: Vec<RefLabel>,
        needle: impl Into<SharedString>,
        color: Hsla,
        actions: LabelActions,
    ) -> Self {
        HistoryRefLabels {
            id: id.into(),
            labels,
            needle: needle.into(),
            color,
            actions,
            more: None,
        }
    }

    pub fn more(mut self, more: MoreLabels) -> Self {
        self.more = Some(more);
        self
    }
}

impl RenderOnce for HistoryRefLabels {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = *cx.palette();
        let ordered: Vec<RefLabel> = ordered_labels(&self.labels, &self.needle)
            .into_iter()
            .cloned()
            .collect();
        let mut ordered = ordered.into_iter();
        let first = ordered.next();
        let rest: Vec<RefLabel> = ordered.collect();
        let id = self.id;
        let color = self.color;
        let actions = self.actions;
        let first = first.map(|label| ref_pill(&id, label, color, &actions, &palette));
        let more =
            (!rest.is_empty()).then(|| more_labels(&id, rest, color, actions, self.more, &palette));
        h_flex()
            .min_w_0()
            .gap_1()
            .overflow_hidden()
            .children(first)
            .children(more)
    }
}

/// A label's pill: its shape, border and text, in its lane's colour.
fn pill(color: Hsla, palette: &Palette) -> gpui_kit::Div {
    h_flex()
        .rounded_sm()
        .border_1()
        .border_color(mixed(color, 0.45, palette.border))
        .py(rems(3. / 15.))
        .text_size(LABEL_TEXT)
        .line_height(LABEL_LINE)
        .text_color(palette.text_soft)
}

fn ref_pill(
    row: &ElementId,
    label: RefLabel,
    color: Hsla,
    actions: &LabelActions,
    palette: &Palette,
) -> AnyElement {
    let branch = match &label {
        RefLabel::Branch(branch) => Some(branch.clone()),
        _ => None,
    };
    let current = branch.as_ref().is_some_and(|branch| branch.current);
    let menu_open = branch
        .as_ref()
        .is_some_and(|branch| actions.menu_open_for.as_deref() == Some(branch.full_name.as_str()));
    let title: SharedString = label_title(&label).into();
    let name: SharedString = label.name().to_owned().into();
    let key = match &branch {
        Some(branch) => branch.full_name.clone(),
        None => format!("{label:?}"),
    };
    let icon = |name: IconName| Icon::new(name).size(LABEL_ICON).flex_none();
    let target = label_target(&label);
    let actions = actions.clone();
    let pill = pill(color, palette)
        .id(part(row, key))
        .role(Role::Label)
        .aria_label(name.clone())
        .test_support()
        .min_w_0()
        .gap(rems(3. / 15.))
        .px(rems(5. / 15.))
        .bg(tinted(color, if menu_open { 0.34 } else { 0.16 }))
        .when(current, |this| {
            this.font_weight(FontWeight(720.)).text_color(palette.text)
        })
        .when(menu_open, |this| this.text_color(palette.text))
        // Not over its menu, while that's open.
        .when(!menu_open, |this| {
            this.tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
        })
        .on_click(move |event, window, cx| {
            if event.click_count() != 2 {
                return;
            }
            match &target {
                Some(LabelTarget::Switch(reference)) => {
                    (actions.switch)(reference.clone(), window, cx)
                }
                Some(LabelTarget::OpenWorktree(path)) => {
                    (actions.open_worktree)(path.clone(), window, cx)
                }
                None => {}
            }
        })
        .when(current, |this| this.child(icon(IconName::Check)))
        .when(matches!(label, RefLabel::Tag { .. }), |this| {
            this.child(icon(IconName::Tag))
        })
        .when(matches!(label, RefLabel::Worktree(_)), |this| {
            this.child(icon(IconName::FolderGit2))
        })
        .child(div().min_w_0().truncate().child(name))
        .when_some(branch.clone(), |this, branch| {
            this.when(branch.local, |this| this.child(icon(IconName::Laptop)))
                .when(!branch.remotes.is_empty(), |this| {
                    this.child(icon(IconName::Cloud))
                })
                .when(branch.worktree.is_some(), |this| {
                    this.child(icon(IconName::FolderGit2))
                })
        });
    match branch {
        Some(branch) => {
            let reference: SharedString = branch.full_name.into();
            pill.context_menu(move |menu, window, cx| {
                branch_menu(menu_frame(menu), reference.clone(), window, cx)
            })
            .into_any_element()
        }
        None => pill.into_any_element(),
    }
}

/// The count of the labels a row has no room for, listing them under it while it's hovered: on
/// the popover's surface, as menus and popovers have it, above the history.
fn more_labels(
    row: &ElementId,
    rest: Vec<RefLabel>,
    color: Hsla,
    actions: LabelActions,
    more: Option<MoreLabels>,
    palette: &Palette,
) -> AnyElement {
    let count = MoreCount {
        id: part(row, "more"),
        count: rest.len(),
        color,
        border: palette.border,
        text: palette.text_soft,
        on_hover: more.as_ref().map(|more| Rc::clone(&more.on_hover)),
        open: more.as_ref().is_some_and(|more| more.open),
    };
    let list = more.filter(|more| more.open).map(|more| {
        let on_hover = Rc::clone(&more.on_hover);
        let pills = rest
            .into_iter()
            .map(|label| ref_pill(row, label, color, &actions, palette));
        // Under the count, its leading edges in line (`placement="bottom-start"`).
        div().absolute().left_0().top_full().child(
            deferred(
                anchored().snap_to_window_with_margin(px(8.)).child(
                    v_flex()
                        .id(part(row, "more-labels"))
                        .test_support()
                        .occlude()
                        .mt_1()
                        .max_w(rems(320. / 15.))
                        .items_start()
                        .gap_1()
                        .p_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(palette.border)
                        .bg(palette.panel_raised)
                        .shadow_lg()
                        .on_hover(move |hovered, window, cx| on_hover(*hovered, window, cx))
                        .children(pills),
                ),
            )
            .with_priority(1),
        )
    });
    div()
        .relative()
        .flex_none()
        .child(count)
        .children(list)
        .into_any_element()
}

/// The `+N` of a row's labels, which lists the rest while it's hovered.
#[derive(IntoElement)]
struct MoreCount {
    id: ElementId,
    count: usize,
    color: Hsla,
    border: Hsla,
    text: Hsla,
    on_hover: Option<OnHover>,
    /// Whether the rest are listed.
    open: bool,
}

impl RenderOnce for MoreCount {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let label: SharedString = format!("+{}", self.count).into();
        h_flex()
            .id(self.id)
            .role(Role::Label)
            .aria_label(label.clone())
            .test_support()
            .flex_none()
            .rounded_sm()
            .border_1()
            .border_color(mixed(self.color, 0.45, self.border))
            .py(rems(3. / 15.))
            .px(rems(4. / 15.))
            .text_size(LABEL_TEXT)
            .line_height(LABEL_LINE)
            .text_color(self.text)
            .font_weight(FontWeight(680.))
            .when(self.open, |this| this.bg(tinted(self.color, 0.16)))
            .when_some(self.on_hover, |this, on_hover| {
                this.on_hover(move |hovered, window, cx| on_hover(*hovered, window, cx))
            })
            .child(label)
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::features::history::schema::{CommitRef, CommitRefKind};
    use gitto_ui::git::ref_labels::{Checkouts, to_ref_labels};

    use super::*;

    fn labels(refs: &[(CommitRefKind, &str, &str)]) -> Vec<RefLabel> {
        let refs: Vec<CommitRef> = refs
            .iter()
            .map(|(kind, name, full_name)| CommitRef {
                kind: *kind,
                name: (*name).into(),
                full_name: (*full_name).into(),
                current: None,
            })
            .collect();
        to_ref_labels(&refs, "e5", &Checkouts::default())
    }

    // `ref-labels.test.tsx`: what double-clicking a label switches to.
    #[test]
    fn switches_to_a_local_branch_thats_double_clicked() {
        let labels = labels(&[
            (CommitRefKind::Local, "feature", "refs/heads/feature"),
            (
                CommitRefKind::Remote,
                "origin/feature",
                "refs/remotes/origin/feature",
            ),
        ]);
        assert_eq!(labels.len(), 1);
        assert_eq!(
            label_target(&labels[0]),
            Some(LabelTarget::Switch("refs/heads/feature".into()))
        );
    }

    #[test]
    fn switches_to_a_remote_branch_without_a_local_copy_here() {
        let labels = labels(&[(
            CommitRefKind::Remote,
            "origin/feature",
            "refs/remotes/origin/feature",
        )]);
        assert_eq!(
            label_target(&labels[0]),
            Some(LabelTarget::Switch("refs/remotes/origin/feature".into()))
        );
    }

    #[test]
    fn doesnt_switch_to_a_tag_or_a_detached_head() {
        let tag = labels(&[(CommitRefKind::Tag, "v1", "refs/tags/v1")]);
        let head = labels(&[(CommitRefKind::Head, "HEAD", "HEAD")]);
        assert_eq!(label_target(&tag[0]), None);
        assert_eq!(label_target(&head[0]), None);
    }

    #[test]
    fn puts_the_label_the_search_matches_first() {
        let labels = labels(&[
            (CommitRefKind::Local, "main", "refs/heads/main"),
            (CommitRefKind::Tag, "v1", "refs/tags/v1"),
        ]);
        let names = |needle| -> Vec<&str> {
            ordered_labels(&labels, needle)
                .into_iter()
                .map(RefLabel::name)
                .collect()
        };
        assert_eq!(names(""), ["main", "v1"]);
        assert_eq!(names("v1"), ["v1", "main"]);
        assert_eq!(names("nothing"), ["main", "v1"]);
    }

    #[test]
    fn says_what_double_clicking_a_branch_does() {
        let labels = labels(&[
            (CommitRefKind::Local, "feature", "refs/heads/feature"),
            (CommitRefKind::Tag, "v1", "refs/tags/v1"),
        ]);
        assert_eq!(
            label_title(&labels[0]),
            format!(
                "{}\nDouble-click to switch to it",
                describe_ref_label(&labels[0])
            )
        );
        assert_eq!(label_title(&labels[1]), describe_ref_label(&labels[1]));
    }
}
