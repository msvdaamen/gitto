//! Gitto's desktop app: the GPUI UI, rebuilt from `apps/ui` (SolidJS), calling the Rust crates
//! directly. Laid out as `apps/ui/src` is:
//!
//! - [`views`]: the pages (`routes/`), and the window's shell around them.
//! - [`components`]: what pages are made of (`components/`), with the primitives in
//!   [`components::ui`].
//! - [`data`]: what's loaded from git and the database, and the changes to a repository, as
//!   entities (`git/queries/` and TanStack Query).
//! - [`state`]: what outlives a view (the router), and the hooks GPUI needs as state (`hooks/`).
//! - [`services`]: what's started at launch (`apps/electron/src/main.ts`): the database, the
//!   repositories, git, the system.
//! - [`theme`]: `index.css`, as a GPUI Kit theme.
//! - [`util`]: pure helpers (`lib/*.ts`); the rest of them, and `git/*.ts`, are in `gitto-ui`.

// GPUI's derive macros name `gpui`: it's the Kit, which re-exports it.
extern crate gpui_kit as gpui;

pub mod app;
pub mod assets;
pub mod components;
pub mod data;
pub mod identity;
pub mod services;
pub mod state;
pub mod theme;
pub mod util;
pub mod views;

#[cfg(test)]
pub(crate) mod test_support;

pub use crate::app::run;
