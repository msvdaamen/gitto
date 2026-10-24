//! The repository's history (`components/repository/history/`): one module per TSX file, named
//! after it. [`history_table::HistoryTable`] is what the page shows.

pub mod author_tooltip;
pub mod columns;
pub mod commit_menu;
pub mod commit_row;
pub mod history_graph;
pub mod history_option;
pub mod history_table;
pub mod ref_labels;
pub mod reword_dialog;
pub mod stash_row;
pub mod wip_row;

#[cfg(test)]
mod tests;
