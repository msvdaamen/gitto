//! Gitto app's primitives and theme, on GPUI Kit: [`ui`] (`components/ui/`) and [`theme`]
//! (`index.css`). They know of the app's data only what they show (an error, a saved width), and
//! nothing of git's repositories or the app's pages.

// GPUI's derive macros name `gpui`: it's the Kit, which re-exports it.
extern crate gpui_kit as gpui;

pub mod theme;
pub mod ui;

#[cfg(test)]
pub(crate) mod test_support;
