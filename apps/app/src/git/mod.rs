//! The few pieces of `apps/ui/src/git/*.ts` the shell needs, until `gitto-ui` lands.
//!
//! That pure logic (`status.ts`, `rows.ts`, `diff-source.ts`, `graph.ts`…) is being ported to the
//! `gitto-ui` crate (`crates/ui`) by another change. Once it's in the tree, uncomment it in
//! Cargo.toml, use its versions of these, and delete this module: each item here is named as
//! there, so that's a change of import.

pub mod changes;
pub mod diff_source;
pub mod patch;
pub mod rows;
pub mod status;
