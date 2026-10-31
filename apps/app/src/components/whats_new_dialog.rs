//! What changed in Gitto. Ported from `components/whats-new-dialog.tsx`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FontWeight, Subscription, Window, div};

use crate::util::format::{capitalize, short_date};
use gitto_app_data::services::system::{Change, ChangeType, UpdateChannel, VersionChanges};
use gitto_app_data::state::whats_new::WhatsNew;
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::dialog::LIST_WIDTH;
use gitto_app_kit::ui::{Badge, DialogHeading, DialogId, IconName, Tone};

/// How a change is marked, in the order changes are listed: breaking ones first, whatever their
/// type, then what's new, fixed and faster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    Breaking,
    New,
    Fixed,
    Faster,
}

impl ChangeKind {
    pub fn of(change: &Change) -> Self {
        if change.breaking {
            ChangeKind::Breaking
        } else if change.kind == ChangeType::Fix {
            ChangeKind::Fixed
        } else if change.kind == ChangeType::Perf {
            ChangeKind::Faster
        } else {
            ChangeKind::New
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Breaking => "Breaking",
            ChangeKind::New => "New",
            ChangeKind::Fixed => "Fixed",
            ChangeKind::Faster => "Faster",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            ChangeKind::Breaking => Tone::Coral,
            ChangeKind::New => Tone::Purple,
            ChangeKind::Fixed => Tone::Mint,
            ChangeKind::Faster => Tone::Blue,
        }
    }
}

/// A version's changes, in the order they're listed (see [`ChangeKind`]).
pub fn sorted_changes(version: &VersionChanges) -> Vec<Change> {
    let mut changes = version.changes.clone();
    changes.sort_by_key(ChangeKind::of);
    changes
}

/// What changed for users in each version since the last one they saw, the newest first: shown
/// once Gitto has updated (asked for when the shell starts), and from its version in the footer.
/// Closing it marks them as seen. An entity the shell keeps, which opens the dialog on the window
/// while `WhatsNew` has something to show.
pub struct WhatsNewDialog {
    whats_new: Entity<WhatsNew>,
    /// The dialog, while it's open.
    dialog: Option<DialogId>,
    _subscription: Subscription,
}

impl WhatsNewDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let whats_new = WhatsNew::global(cx);
        let subscription = cx.observe_in(&whats_new, window, |this, _, window, cx| {
            this.sync(window, cx);
        });
        whats_new.update(cx, |whats_new, cx| whats_new.show_unseen(cx));
        WhatsNewDialog {
            whats_new,
            dialog: None,
            _subscription: subscription,
        }
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.whats_new.read(cx).shown().is_some();
        if shown && self.dialog.is_none() {
            let whats_new = self.whats_new.clone();
            self.dialog = Some(DialogId::open(window, cx, move |_, dialog, window, cx| {
                render_dialog(&whats_new, dialog, window, cx)
            }));
        } else if !shown && let Some(dialog) = self.dialog.take() {
            dialog.close(window, cx);
        }
        window.refresh();
    }
}

fn render_dialog(
    whats_new: &Entity<WhatsNew>,
    dialog: Dialog,
    window: &Window,
    cx: &App,
) -> Dialog {
    let Some(shown) = whats_new.read(cx).shown().cloned() else {
        return dialog;
    };
    let palette = *cx.palette();
    let close = whats_new.clone();
    let confirm = whats_new.clone();
    let cancel = whats_new.clone();
    dialog
        .w(LIST_WIDTH.to_pixels(window.rem_size()))
        .close_button(false)
        // Every way out (Enter, Escape, a click outside, Got it) only closes `WhatsNew`: the
        // dialog follows it (see `sync`). Closing it here too would close it twice, and the
        // second time the dialog under it, the one saying git's too old, say.
        .on_ok(move |_, _, cx| {
            confirm.update(cx, |whats_new, cx| whats_new.close(cx));
            false
        })
        .on_cancel(move |_, _, cx| {
            cancel.update(cx, |whats_new, cx| whats_new.close(cx));
            false
        })
        .child(
            DialogHeading::new(IconName::Sparkles, Tone::Purple, "What's new in Gitto")
                .description(div().child(if shown.unseen {
                    "Gitto has updated. Here's what changed since you last looked."
                } else {
                    "What changed in each version, the newest first."
                })),
        )
        .child(
            v_flex()
                .id("whats-new-versions")
                .mt_4()
                .max_h_96()
                .overflow_y_scroll()
                .border_y_1()
                .border_color(palette.border)
                .py_1()
                .map(|this| {
                    if shown.versions.is_empty() {
                        this.child(
                            div()
                                .my_4()
                                .text_sm()
                                .text_color(palette.muted)
                                .child("Nothing to show: this build was made without a changelog."),
                        )
                    } else {
                        this.children(
                            shown
                                .versions
                                .iter()
                                .map(|version| render_version(version, cx)),
                        )
                    }
                }),
        )
        .footer(
            DialogFooter::new().child(
                Button::new("got-it")
                    .primary()
                    .label("Got it")
                    .on_click(move |_, _, cx| {
                        close.update(cx, |whats_new, cx| whats_new.close(cx))
                    }),
            ),
        )
}

fn render_version(version: &VersionChanges, cx: &App) -> impl IntoElement + use<> {
    let palette = *cx.palette();
    let nightly = UpdateChannel::of(&version.version) == UpdateChannel::Nightly;
    let changes = sorted_changes(version);
    v_flex()
        .border_b_1()
        .border_color(palette.border)
        .py_3()
        .child(
            h_flex()
                .gap_2()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(if nightly {
                    "Nightly".to_owned()
                } else {
                    format!("Gitto {}", version.version)
                })
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(palette.faint)
                        .child(short_date(&version.date)),
                )
                .when(nightly, |this| {
                    this.child(
                        div()
                            .ml_auto()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(palette.faint)
                            .child(format!("v{}", version.version)),
                    )
                }),
        )
        .child(
            v_flex()
                .mt_2()
                .gap_2()
                .children(changes.into_iter().map(|change| {
                    let kind = ChangeKind::of(&change);
                    h_flex()
                        .items_start()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .w_16()
                                .flex_none()
                                .child(Badge::new(kind.label()).tone(kind.tone())),
                        )
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    h_flex()
                                        .flex_wrap()
                                        .when_some(change.scope.clone(), |this, scope| {
                                            this.child(
                                                div()
                                                    .text_color(palette.muted)
                                                    .child(format!("{}: ", capitalize(&scope))),
                                            )
                                        })
                                        .child(
                                            div()
                                                .text_color(palette.text)
                                                .child(capitalize(&change.description)),
                                        ),
                                )
                                .when_some(change.detail.clone(), |this, detail| {
                                    this.child(
                                        div().mt_0p5().text_color(palette.text_soft).child(detail),
                                    )
                                }),
                        )
                })),
        )
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::future::BoxFuture;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AnyWindowHandle, AppContext as _, IntoElement, Render, TestAppContext};

    use super::*;
    use crate::test_support;
    use gitto_app_data::services::system::{SystemContext, WhatsNew as WhatsNewSource};

    /// The changelog as a test has it, counting the times it's told it's seen.
    struct Changelog {
        unseen: Vec<VersionChanges>,
        all: Vec<VersionChanges>,
        seen: AtomicUsize,
    }

    impl WhatsNewSource for Changelog {
        fn unseen(&self) -> BoxFuture<'static, io::Result<Vec<VersionChanges>>> {
            let unseen = self.unseen.clone();
            Box::pin(async move { Ok(unseen) })
        }

        fn all(&self) -> Vec<VersionChanges> {
            self.all.clone()
        }

        fn seen(&self) -> BoxFuture<'static, io::Result<()>> {
            self.seen.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    fn change(kind: ChangeType, scope: Option<&str>, breaking: bool, description: &str) -> Change {
        Change {
            kind,
            scope: scope.map(str::to_owned),
            breaking,
            description: description.to_owned(),
            detail: None,
            pr: None,
            commit: "a".to_owned(),
        }
    }

    fn versions() -> Vec<VersionChanges> {
        vec![
            VersionChanges {
                version: "1.2.4-nightly29860000".into(),
                date: "2026-10-12".into(),
                changes: vec![
                    change(ChangeType::Fix, Some("diff"), false, "keep the scroll"),
                    Change {
                        detail: Some("Open one by double-clicking it.".into()),
                        pr: Some(74),
                        ..change(ChangeType::Feat, None, false, "add worktrees")
                    },
                ],
            },
            VersionChanges {
                version: "1.2.3".into(),
                date: "2026-10-10".into(),
                changes: vec![change(ChangeType::Perf, None, true, "load faster")],
            },
        ]
    }

    struct Host {
        _dialog: Entity<WhatsNewDialog>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn open(
        changelog: Arc<Changelog>,
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, AnyWindowHandle) {
        let dir = test_support::init(cx, |services| {
            let system = SystemContext {
                changelog: changelog.clone(),
                ..services.system().clone()
            };
            services.with_system(system)
        });
        let (window, _) = test_support::open(cx, |window, cx| Host {
            _dialog: cx.new(|cx| WhatsNewDialog::new(window, cx)),
        });
        cx.run_until_parked();
        (dir, window)
    }

    fn has_dialog(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    #[gpui_kit::test]
    fn shows_what_changed_since_the_last_version_seen_and_remembers_it_seen_once_closed(
        cx: &mut TestAppContext,
    ) {
        let changelog = Arc::new(Changelog {
            unseen: versions(),
            all: Vec::new(),
            seen: AtomicUsize::new(0),
        });
        let (_dir, window) = open(changelog.clone(), cx);
        assert!(has_dialog(window, cx));
        let shown = cx.update(|cx| WhatsNew::global(cx).read(cx).shown().cloned());
        assert!(shown.unwrap().unseen);

        cx.update_window(window, |_, window, cx| window.click("got-it", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(!has_dialog(window, cx));
        assert_eq!(changelog.seen.load(Ordering::SeqCst), 1);
    }

    #[gpui_kit::test]
    fn doesnt_open_with_nothing_new(cx: &mut TestAppContext) {
        let changelog = Arc::new(Changelog {
            unseen: Vec::new(),
            all: versions(),
            seen: AtomicUsize::new(0),
        });
        let (_dir, window) = open(changelog, cx);
        assert!(!has_dialog(window, cx));
    }

    #[gpui_kit::test]
    fn shows_every_version_when_asked(cx: &mut TestAppContext) {
        let changelog = Arc::new(Changelog {
            unseen: Vec::new(),
            all: versions(),
            seen: AtomicUsize::new(0),
        });
        let (_dir, window) = open(changelog, cx);
        cx.update(|cx| WhatsNew::global(cx).update(cx, |whats_new, cx| whats_new.show_all(cx)));
        cx.run_until_parked();
        assert!(has_dialog(window, cx));
        let shown = cx
            .update(|cx| WhatsNew::global(cx).read(cx).shown().cloned())
            .unwrap();
        assert!(!shown.unseen);
        assert_eq!(shown.versions.len(), 2);
    }

    #[gpui_kit::test]
    fn closes_on_enter_and_escape_remembering_it_seen(cx: &mut TestAppContext) {
        for key in ["enter", "escape"] {
            let changelog = Arc::new(Changelog {
                unseen: versions(),
                all: Vec::new(),
                seen: AtomicUsize::new(0),
            });
            let (_dir, window) = open(changelog.clone(), cx);
            assert!(has_dialog(window, cx));

            cx.update_window(window, |_, window, cx| window.press(key, cx))
                .unwrap();
            cx.run_until_parked();
            assert!(!has_dialog(window, cx), "{key}");
            assert!(!cx.update(|cx| WhatsNew::is_open(cx)), "{key}");
            assert_eq!(changelog.seen.load(Ordering::SeqCst), 1, "{key}");
        }
    }

    #[gpui_kit::test]
    fn leaves_the_dialog_under_it_open(cx: &mut TestAppContext) {
        let changelog = Arc::new(Changelog {
            unseen: Vec::new(),
            all: versions(),
            seen: AtomicUsize::new(0),
        });
        let (_dir, window) = open(changelog, cx);
        cx.update_window(window, |_, window, cx| {
            window.open_dialog(cx, |dialog, _, _| dialog.keyboard(false));
        })
        .unwrap();
        for close in ["escape", "got-it"] {
            cx.update(|cx| WhatsNew::global(cx).update(cx, |whats_new, cx| whats_new.show_all(cx)));
            cx.run_until_parked();
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                match close {
                    "escape" => window.press("escape", cx),
                    _ => window.click(close, cx),
                }
            })
            .unwrap();
            cx.run_until_parked();
            assert!(!cx.update(|cx| WhatsNew::is_open(cx)), "{close}");
            assert!(has_dialog(window, cx), "{close}");
        }
    }

    #[test]
    fn lists_whats_new_then_fixed_and_marks_breaking_changes() {
        let versions = versions();
        let labels = |version: &VersionChanges| -> Vec<(&'static str, String)> {
            sorted_changes(version)
                .iter()
                .map(|change| (ChangeKind::of(change).label(), change.description.clone()))
                .collect()
        };
        assert_eq!(
            labels(&versions[0]),
            [
                ("New", "add worktrees".to_owned()),
                ("Fixed", "keep the scroll".to_owned())
            ]
        );
        assert_eq!(
            labels(&versions[1]),
            [("Breaking", "load faster".to_owned())]
        );
    }
}
