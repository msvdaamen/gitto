//! Gitto app's primitives and theme, on GPUI Kit: [`ui`] (`components/ui/`) and [`theme`]
//! (`index.css`), with [`delayed`], the flag they show what's loading with. They build on GPUI
//! Kit alone: they know nothing of the app's data or git's repositories (an error is its message,
//! a saved theme or width is the app's to keep), so they build alongside the data layer.

// GPUI's derive macros name `gpui`: it's the Kit, which re-exports it.
extern crate gpui_kit as gpui;

pub mod delayed;
pub mod theme;
pub mod ui;

#[cfg(test)]
pub(crate) mod test_support;
