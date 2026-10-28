//! The What's new dialog's state. Ported from `hooks/whats-new.ts`.

use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task};

use crate::services::Services;
use crate::services::system::VersionChanges;

/// What the What's new dialog shows.
#[derive(Debug, Clone, PartialEq)]
pub struct WhatsNewShown {
    pub versions: Vec<VersionChanges>,
    /// Whether it's what changed since the user last saw, or every version, as asked for.
    pub unseen: bool,
}

/// The What's new dialog, opened after an update, or from the app's version in the footer: an
/// entity in a global, which the dialog observes.
#[derive(Default)]
pub struct WhatsNew {
    shown: Option<WhatsNewShown>,
    task: Option<Task<()>>,
}

struct GlobalWhatsNew(Entity<WhatsNew>);

impl Global for GlobalWhatsNew {}

impl WhatsNew {
    /// Sets up the app's (see `state::init`).
    pub fn init(cx: &mut App) {
        let whats_new = cx.new(|_| WhatsNew::default());
        cx.set_global(GlobalWhatsNew(whats_new));
    }

    pub fn global(cx: &App) -> Entity<WhatsNew> {
        cx.global::<GlobalWhatsNew>().0.clone()
    }

    /// What it shows; `None` while it's closed.
    pub fn shown(&self) -> Option<&WhatsNewShown> {
        self.shown.as_ref()
    }

    /// Opens it with what changed since the last version the user saw, if anything did.
    pub fn show_unseen(&mut self, cx: &mut Context<Self>) {
        let unseen = Services::global(cx).system().changelog.unseen();
        self.task = Some(cx.spawn(async move |this, cx| match unseen.await {
            Ok(versions) if !versions.is_empty() => {
                let _ = this.update(cx, |this, cx| {
                    this.shown = Some(WhatsNewShown {
                        versions,
                        unseen: true,
                    });
                    cx.notify();
                });
            }
            Ok(_) => {}
            Err(error) => log::error!("Couldn't read what's new: {error}"),
        }));
    }

    /// Opens it with every version's changes.
    pub fn show_all(&mut self, cx: &mut Context<Self>) {
        let versions = Services::global(cx).system().changelog.all();
        self.shown = Some(WhatsNewShown {
            versions,
            unseen: false,
        });
        cx.notify();
    }

    /// Closes it, and remembers this version's changes as seen.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.shown = None;
        cx.notify();
        let seen = Services::global(cx).system().changelog.seen();
        cx.background_spawn(async move {
            if let Err(error) = seen.await {
                log::error!("Couldn't remember what's new as seen: {error}");
            }
        })
        .detach();
    }

    /// For tests: whether it's open.
    pub fn is_open(cx: &App) -> bool {
        cx.try_global::<GlobalWhatsNew>()
            .is_some_and(|whats_new| whats_new.0.read(cx).shown.is_some())
    }
}
