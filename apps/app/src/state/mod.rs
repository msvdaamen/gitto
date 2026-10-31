//! App state that outlives a view, where TS kept it in a hook's module or a context, and the hooks
//! (`hooks/*.ts`) that GPUI needs as state. One module per hook, named after it:
//!
//! - [`router`]: the page on show (TanStack Router).
//! - [`panel_width`], [`collapsed`], [`diff_style`]: what's remembered across restarts.
//! - [`settled`], [`delayed`]: values that follow another, in time; entities, observed.
//! - [`clock`]: the minute tick relative times follow (`relative-time.ts`).
//! - [`update`], [`whats_new`]: the app's own version and changes.
//! - `theme.ts` is `crate::theme`.
//!
//! Hooks with no counterpart, as GPUI does what they did: `connected.ts` (elements are laid out
//! before they're measured), `lingering.ts` (a dialog is built with what it's about, and keeps
//! it while it closes), `reset-on.ts` (a view resets its state where it changes repository),
//! `window-focus.ts` (`cx.observe_window_activation`), `repositories.ts` (`crate::data`).

pub mod clock;
pub mod collapsed;
pub mod delayed;
pub mod diff_style;
pub mod panel_width;
pub mod router;
pub mod settled;
pub mod update;
pub mod whats_new;

/// Sets up the state the whole app shares, in globals: the clock, the update's status and What's
/// new. Call once the services are set up (`Services` is a global): the update's status starts
/// watching the updater. The router's set up on its own, on the page to start on
/// (`Router::init`).
pub fn init(cx: &mut gpui_kit::App) {
    clock::Clock::init(cx);
    update::UpdateStatus::init(cx);
    whats_new::WhatsNew::init(cx);
}
