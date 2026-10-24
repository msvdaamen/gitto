//! The repository's sidebar (`components/repository/sidebar/`): the refs, stashes and worktrees
//! in collapsible sections ([`ref_list`]), with their menus and dialogs, in the sidebar's frame
//! ([`refs_sidebar`]).

pub mod agent_placeholder;
pub mod drop_stash_dialog;
pub mod ref_list;
pub mod refs_sidebar;
pub mod remove_worktree_dialog;
pub mod sidebar_row;
pub mod sidebar_section;
pub mod stash_menu;
pub mod worktree_menu;

use gpui_kit::{Rems, rems};

/// `px` pixels of the 15px base font, in rems: the sidebar's measures are TS's pixels, kept as
/// rems so they follow the app's zoom.
pub(crate) const fn base_px(px: f32) -> Rems {
    rems(px / 15.)
}
