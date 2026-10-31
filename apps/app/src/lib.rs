//! Gitto's desktop app: the GPUI UI, rebuilt from `apps/ui` (SolidJS), calling the Rust crates
//! directly. Laid out as `apps/ui/src` is:
//!
//! - [`views`]: the pages (`routes/`), and the window's shell around them.
//! - [`components`]: what pages are made of (`components/`).
//! - [`util`]: pure helpers (`lib/*.ts`); the rest of them, and `git/*.ts`, are in `gitto-ui-logic`.
//!
//! The rest is in two crates of its own, which the app builds on:
//!
//! - `gitto-app-data` (`apps/app/data`): the data layer (`git/queries/` and TanStack Query), the
//!   services started at launch (`apps/electron/src/main.ts`), and the state that outlives a view
//!   (the router, and the hooks GPUI needs as state, `hooks/`).
//! - `gitto-app-kit` (`apps/app/kit`): the primitives (`components/ui/`) and the theme
//!   (`index.css`).

// GPUI's derive macros name `gpui`: it's the Kit, which re-exports it.
extern crate gpui_kit as gpui;

pub mod app;
pub mod assets;
pub mod components;
pub mod identity;
pub mod menus;
pub mod single_instance;
pub mod util;
pub mod views;

#[cfg(test)]
pub(crate) mod test_support;

pub use crate::app::run;
