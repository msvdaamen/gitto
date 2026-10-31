//! Gitto's desktop app: the GPUI UI, rebuilt from `apps/ui` (SolidJS), calling the Rust crates
//! directly. Laid out as `apps/ui/src` is; the pages and components come in the next change:
//!
//! - [`data`]: the queries and operations (`git/queries/`, TanStack Query).
//! - [`state`]: what outlives a view (the router), and the hooks GPUI needs as state (`hooks/`).
//! - [`services`]: what's started at launch (`apps/electron/src/main.ts`): the database, the
//!   repositories, git, the system.
//! - [`theme`]: `index.css`, as a GPUI Kit theme.
//! - [`git`] and [`util`]: pure helpers (`git/*.ts`, `lib/*.ts`).

// GPUI's derive macros name `gpui`: it's the Kit, which re-exports it.
extern crate gpui_kit as gpui;

pub mod assets;
pub mod data;
pub mod git;
pub mod services;
pub mod state;
pub mod theme;
pub mod util;
