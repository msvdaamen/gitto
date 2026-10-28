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
        // What changed since the user last saw, if it's still being read, would replace it.
        self.task = None;
        let versions = Services::global(cx).system().changelog.all();
        self.shown = Some(WhatsNewShown {
            versions,
            unseen: false,
        });
        cx.notify();
    }

    /// Closes it, and remembers this version's changes as seen.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        // Nor does what's still being read open it again.
        self.task = None;
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use futures::channel::oneshot;
    use futures::future::BoxFuture;
    use gpui_kit::TestAppContext;

    use super::*;
    use crate::services::system::{self, Change, ChangeType};

    /// What changed, read once the test sends it.
    struct ReadLater(Mutex<Option<oneshot::Receiver<Vec<VersionChanges>>>>);

    impl system::WhatsNew for ReadLater {
        fn unseen(&self) -> BoxFuture<'static, io::Result<Vec<VersionChanges>>> {
            let read = self.0.lock().unwrap().take().expect("read once");
            Box::pin(async move { Ok(read.await.unwrap_or_default()) })
        }

        fn all(&self) -> Vec<VersionChanges> {
            Vec::new()
        }

        fn seen(&self) -> BoxFuture<'static, io::Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    fn version() -> VersionChanges {
        VersionChanges {
            version: "1.2.4".to_owned(),
            date: "2026-10-12".to_owned(),
            changes: vec![Change {
                kind: ChangeType::Feat,
                scope: None,
                breaking: false,
                description: "add it".to_owned(),
                detail: None,
                pr: None,
                commit: "a".to_owned(),
            }],
        }
    }

    /// Sets up What's new on changes read once the returned sender sends them.
    fn init(cx: &mut TestAppContext) -> (tempfile::TempDir, oneshot::Sender<Vec<VersionChanges>>) {
        let dir = tempfile::tempdir().unwrap();
        let (send, read) = oneshot::channel();
        let services = Services::in_memory(dir.path()).unwrap();
        let mut system = services.system().clone();
        system.changelog = Arc::new(ReadLater(Mutex::new(Some(read))));
        cx.update(|cx| {
            cx.set_global(services.with_system(system));
            crate::state::init(cx);
        });
        (dir, send)
    }

    fn shown(cx: &mut TestAppContext) -> Option<WhatsNewShown> {
        cx.update(|cx| WhatsNew::global(cx).read(cx).shown().cloned())
    }

    #[gpui_kit::test]
    fn keeps_every_version_shown_when_whats_unseen_is_read_after(cx: &mut TestAppContext) {
        let (_dir, send) = init(cx);
        let whats_new = cx.update(|cx| WhatsNew::global(cx));
        whats_new.update(cx, |whats_new, cx| whats_new.show_unseen(cx));
        whats_new.update(cx, |whats_new, cx| whats_new.show_all(cx));

        // Called off: nothing reads it any more.
        let _ = send.send(vec![version()]);
        cx.run_until_parked();
        assert_eq!(
            shown(cx),
            Some(WhatsNewShown {
                versions: Vec::new(),
                unseen: false
            })
        );
    }

    #[gpui_kit::test]
    fn stays_closed_when_whats_unseen_is_read_after(cx: &mut TestAppContext) {
        let (_dir, send) = init(cx);
        let whats_new = cx.update(|cx| WhatsNew::global(cx));
        whats_new.update(cx, |whats_new, cx| whats_new.show_unseen(cx));
        whats_new.update(cx, |whats_new, cx| whats_new.close(cx));

        let _ = send.send(vec![version()]);
        cx.run_until_parked();
        assert_eq!(shown(cx), None);
    }

    #[gpui_kit::test]
    fn opens_with_whats_unseen(cx: &mut TestAppContext) {
        let (_dir, send) = init(cx);
        let whats_new = cx.update(|cx| WhatsNew::global(cx));
        whats_new.update(cx, |whats_new, cx| whats_new.show_unseen(cx));

        send.send(vec![version()]).unwrap();
        cx.run_until_parked();
        assert_eq!(
            shown(cx),
            Some(WhatsNewShown {
                versions: vec![version()],
                unseen: true
            })
        );
    }
}
