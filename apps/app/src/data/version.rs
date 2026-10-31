//! The installed git, and whether Gitto works with it. Ported from `git/queries/version.ts`.

use futures::FutureExt as _;
use gitto_git::GitInstall;
use gpui_kit::{App, Context};

use super::client::{QueryOptions, StaleTime};
use super::keys;
use super::query::Query;
use crate::services::Services;

/// The installed git, and whether Gitto works with it. One that does is only checked once; one
/// that doesn't is checked again whenever the window gets focus, e.g. back from installing a newer
/// one.
pub fn git_version_options(cx: &App) -> QueryOptions<GitInstall> {
    let git = Services::global(cx).git().clone();
    QueryOptions::new(keys::git_version(), move |_, _| {
        let git = git.clone();
        async move { Ok(git.version().check().await) }.boxed()
    })
    .stale_time_with(|install| match install {
        Some(install) if install.supported => StaleTime::Infinite,
        _ => StaleTime::Zero,
    })
}

/// The git version's query (`useGitVersion`).
pub fn git_version_query<V: 'static>(cx: &mut Context<V>) -> Query<GitInstall> {
    Query::new(git_version_options(cx), cx)
}
