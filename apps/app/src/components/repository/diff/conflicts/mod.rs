//! A conflicted file's conflicts, resolved in the file's view (`conflict_resolution`): one by one
//! in the conflicts' viewer (`conflict_viewer`, drawing the rows `conflict_rows` lays out from what
//! `conflict_diff` reads), or by keeping a side whole (`conflict_sides`). The file is edited whole
//! in place with `conflict_whole_file`; marking it resolved with markers left asks first
//! (`markers_dialog`).

pub mod conflict_diff;
pub mod conflict_resolution;
pub mod conflict_rows;
pub mod conflict_sides;
pub mod conflict_viewer;
pub mod conflict_whole_file;
pub mod markers_dialog;
