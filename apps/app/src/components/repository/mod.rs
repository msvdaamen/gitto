//! A repository's page's parts (`components/repository/`). Each area is its own file, named after
//! the TSX it replaces; the page (`views::repository`) lays them out and wires them together.
//!
//! An area that keeps state is an entity, made with the repository's id, told when the page
//! shows another repository (`set_repository`), and talking back through its events. The ones
//! still to be built are **stubs** that render a placeholder, with their final interface.

pub mod details;
pub mod diff;
pub mod history;
pub mod sidebar;

pub mod branch_button;
pub mod operation_bar;
pub mod pull_button;
pub mod push_button;
pub mod stash_buttons;
pub mod toolbar;
pub mod toolbar_button;

use std::rc::Rc;

use gpui_kit::{App, Task};

/// What a change to the working tree waits for, resolving to whether to go on: e.g. the edits to
/// the file on show saved, or the user saying what to do with them (TS's `beforeChange` and
/// `mayLeaveFile`).
pub type BeforeChange = Rc<dyn Fn(&mut App) -> Task<bool>>;
