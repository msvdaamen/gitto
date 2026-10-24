//! The running Gitto's version, and the update it's getting. Ported from `hooks/update.ts`.

use futures::StreamExt as _;
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task};

use crate::services::Services;
use crate::services::system::UpdateState;

/// What the updater says (see `Updates::watch`), kept up to date: one for the app, which the
/// footer and the notifications observe.
pub struct UpdateStatus {
    state: Option<UpdateState>,
    _watch: Task<()>,
}

struct GlobalUpdateStatus(Entity<UpdateStatus>);

impl Global for GlobalUpdateStatus {}

impl UpdateStatus {
    /// The app's, watching the updater from the first time it's asked for.
    pub fn global(cx: &mut App) -> Entity<UpdateStatus> {
        if let Some(status) = cx.try_global::<GlobalUpdateStatus>() {
            return status.0.clone();
        }
        let mut states = Services::global(cx).system().updates.watch();
        let status = cx.new(|cx: &mut Context<UpdateStatus>| UpdateStatus {
            state: None,
            _watch: cx.spawn(async move |this, cx| {
                while let Some(next) = states.next().await {
                    let updated = this.update(cx, |status, cx| {
                        status.state = Some(next);
                        cx.notify();
                    });
                    if updated.is_err() {
                        break;
                    }
                }
            }),
        });
        cx.set_global(GlobalUpdateStatus(status.clone()));
        status
    }

    /// The version and the update; `None` until the updater says.
    pub fn state(&self) -> Option<&UpdateState> {
        self.state.as_ref()
    }

    /// Restarts Gitto into the update, once it's ready.
    pub fn install(cx: &App) {
        Services::global(cx).system().updates.install();
    }
}
