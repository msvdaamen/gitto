//! The name the user commits with. Ported from `git/queries/user.ts`.

use futures::FutureExt as _;
use gitto_git::features::user::commands::get_user_name;
use gpui_kit::Context;

use super::client::{QueryOptions, RefetchOnFocus, StaleTime};
use super::keys;
use super::query::Query;

/// The name the user commits with, from git's global config; `None` if it isn't set.
///
/// Not under `keys::all()`, so it doesn't get their defaults (see `QueryClient::init`): without
/// these, `git config user.name` would run again every time the window got focus, for a name
/// that's read once and all but never changes. Until it's set, though: someone without one can set
/// it in a terminal, then come back to be greeted by it.
pub fn user_name_options() -> QueryOptions<Option<String>> {
    QueryOptions::new(keys::git_user_name(), |_, _| {
        async { Ok(get_user_name().await) }.boxed()
    })
    .stale_time(StaleTime::Infinite)
    .refetch_on_window_focus_with(|name| match name {
        Some(None) => RefetchOnFocus::Always,
        _ => RefetchOnFocus::Never,
    })
}

/// The user's name's query (`useUserName`).
pub fn user_name_query<V: 'static>(cx: &mut Context<V>) -> Query<Option<String>> {
    Query::new(user_name_options(), cx)
}
