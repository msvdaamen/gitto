//! A repository's page's parts (`components/repository/`). Each area is its own file, named after
//! the TSX it replaces; the page (`views::repository`) lays them out and wires them together.
//!
//! An area that keeps state is an entity, made with the repository's id, and talking back through
//! its events. When the page shows another repository, it makes the toolbar and the operation
//! bar again, for that one, so nothing of the other's is left in them; the areas that keep their
//! place (the sidebar, the history, the details, and the branch menus they share) are told
//! (`set_repository`).

pub mod details;
pub mod diff;
pub mod history;
pub mod sidebar;

pub mod add_worktree_dialog;
pub mod branch_button;
pub mod branch_form;
pub mod branch_menu;
pub mod commit_message;
pub mod create_branch_dialog;
pub mod delete_branch_dialog;
pub mod operation_bar;
pub mod pull_button;
pub mod push_button;
pub mod row_menu;
pub mod stash_buttons;
pub mod toolbar;
pub mod toolbar_button;

#[cfg(test)]
pub(crate) mod test_support;

use std::rc::Rc;

use gpui_kit::{App, Task};

/// What a change to the working tree waits for, resolving to whether to go on: e.g. the edits to
/// the file on show saved, or the user saying what to do with them (TS's `beforeChange` and
/// `mayLeaveFile`).
pub type BeforeChange = Rc<dyn Fn(&mut App) -> Task<bool>>;
