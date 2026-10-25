//! App state that outlives a view, where TS kept it in a hook's module or a context, and the hooks
//! (`hooks/*.ts`) that GPUI needs as state. One module per hook, named after it:
//!
//! - [`router`]: the page on show (TanStack Router).
//! - [`panel_width`], [`collapsed`], [`diff_style`]: what's remembered across restarts.
//! - [`settled`], [`delayed`]: values that follow another, in time; entities, observed.
//! - [`clock`]: the minute tick relative times follow (`relative-time.ts`).
//! - [`update`], [`whats_new`]: the app's own version and changes.
//! - [`lingering`]: a value that lingers once it's gone; a plain value a view keeps (most dialogs
//!   are built with what they're about and keep it while they close).
//! - `theme.ts` is `gitto_app_kit::theme`.
//!
//! Hooks with no counterpart, as GPUI does what they did: `connected.ts` (elements are laid out
//! before they're measured), `reset-on.ts` (a view resets its state where it changes repository,
//! in `set_repository`), `window-focus.ts` (`cx.observe_window_activation` and
//! `window.is_window_active()`), `repositories.ts` (`crate::data::repositories`).

pub mod clock;
pub mod collapsed;
pub mod delayed;
pub mod diff_style;
pub mod lingering;
pub mod panel_width;
pub mod router;
pub mod settled;
pub mod update;
pub mod whats_new;
