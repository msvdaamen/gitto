//! The pages (`routes/`), and the window's shell around them.
//!
//! - [`root`]: the window ([`root::AppShell`]): header, page, footer, and the dialogs over them.
//! - [`home`], [`settings`], [`repository`]: the pages, one per route, laid out as the route
//!   was; their areas are `crate::components`'.
//!
//! Navigating is `state::router::Router::navigate`.

pub mod home;
pub mod repository;
pub mod root;
pub mod settings;
