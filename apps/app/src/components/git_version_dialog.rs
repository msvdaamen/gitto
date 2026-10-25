//! Says Gitto can't work with the installed git. Ported from
//! `components/git-version-dialog.tsx`.

use gitto_git::GitInstall;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, FontWeight, HighlightStyle, Role, StyledText, Subscription,
    TestSupportExt as _, Window, div,
};

use gitto_app_data::data::{AppChanged, AppData, AppPart, Repositories};
use gitto_app_kit::theme::PaletteExt as _;
use gitto_app_kit::ui::dialog::CONFIRM_WIDTH;
use gitto_app_kit::ui::{DialogHeading, Icon, IconName, Tone};

pub const GIT_DOWNLOAD_URL: &str = "https://git-scm.com/downloads";

/// Says, until it's fixed, that Gitto can't work with the installed git: it's too old, or isn't
/// installed at all. It can't be dismissed, as nothing works meanwhile (see `GitApi::open`);
/// checking again closes it once git has been updated, and loads again what failed for want of
/// it. An entity the shell keeps, which opens the dialog on the window when it's needed.
pub struct GitVersionDialog {
    data: Entity<AppData>,
    /// Whether a check the user asked for found the same git as before.
    unchanged: bool,
    checking: bool,
    open: bool,
    was_supported: Option<bool>,
    _subscription: Subscription,
}

impl GitVersionDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let data = AppData::global(cx);
        data.update(cx, |data, cx| data.show(AppPart::GitVersion, cx));
        let subscription =
            cx.subscribe_in(&data, window, |this, _, AppChanged(part), window, cx| {
                if *part == AppPart::GitVersion {
                    this.sync(window, cx);
                }
            });
        GitVersionDialog {
            data,
            unchanged: false,
            checking: false,
            open: false,
            was_supported: None,
            _subscription: subscription,
        }
    }

    fn installed(&self, cx: &App) -> Option<GitInstall> {
        self.data.read(cx).git_version().data().cloned()
    }

    /// Whether it's showing.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the dialog as the installed git says, and loads again what failed once
    /// it's supported.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let supported = self.installed(cx).map(|install| install.supported);
        if supported == Some(true) && self.was_supported == Some(false) {
            Repositories::reload_all(cx);
            self.data.update(cx, |data, cx| data.reload_shown(cx));
        }
        if supported.is_some() {
            self.was_supported = supported;
        }
        let needed = supported == Some(false);
        if needed && !self.open {
            self.open = true;
            let this = cx.entity();
            window.open_dialog(cx, move |dialog, window, cx| {
                render_dialog(&this, dialog, window, cx)
            });
        } else if !needed && self.open {
            self.open = false;
            window.close_dialog(cx);
        }
        window.refresh();
        cx.notify();
    }

    /// Checks the installed git again, saying if it's the same one.
    pub fn check_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.installed(cx).and_then(|install| install.version);
        self.unchanged = false;
        self.checking = true;
        window.refresh();
        let checked = self
            .data
            .update(cx, |data, cx| data.reload(AppPart::GitVersion, cx));
        cx.spawn_in(window, async move |this, cx| {
            if let Some(checked) = checked {
                let _ = checked.await;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let after = this.installed(cx);
                this.checking = false;
                this.unchanged =
                    after.is_some_and(|install| !install.supported && install.version == before);
                window.refresh();
                cx.notify();
            });
        })
        .detach();
    }
}

/// `text`, with `bold` parts in bold, in the text's color.
fn with_bold(text: String, bold: &[&str], cx: &App) -> StyledText {
    let mut highlights = Vec::new();
    let mut from = 0;
    for part in bold {
        if let Some(at) = text[from..].find(part) {
            let start = from + at;
            highlights.push((
                start..start + part.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    color: Some(cx.palette().text),
                    ..HighlightStyle::default()
                },
            ));
            from = start + part.len();
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

/// What the dialog says about the installed git: its title, why Gitto can't work with it (with
/// the versions to bold), and, after a check that found the same git, that it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitVersionCopy {
    pub title: &'static str,
    pub explanation: String,
    pub bold: Vec<String>,
    pub still: Option<String>,
}

impl GitVersionCopy {
    pub fn of(install: &GitInstall, unchanged: bool) -> Self {
        let required = &install.required;
        let (title, explanation, bold) = match &install.version {
            Some(version) => (
                "Gitto needs a newer version of Git",
                format!(
                    "Git {version} is installed, but Gitto needs Git {required} or newer. It \
                     relies on features older versions of Git don't have, so it can't open your \
                     repositories until Git is updated."
                ),
                vec![version.clone(), required.clone()],
            ),
            None => (
                "Gitto couldn't find Git",
                format!(
                    "Gitto uses Git to read and change your repositories, but Git isn't \
                     installed, or isn't on your PATH. Install Git {required} or newer to get \
                     started."
                ),
                Vec::new(),
            ),
        };
        let still = unchanged.then(|| match &install.version {
            None => "Still no Git found. If you've just installed it, make sure it's on your \
                     PATH."
                .to_owned(),
            Some(version) => format!(
                "Still finding Git {version}. If you've installed a newer one, make sure it's \
                 the first git on your PATH."
            ),
        });
        GitVersionCopy {
            title,
            explanation,
            bold,
            still,
        }
    }
}

fn render_dialog(
    this: &Entity<GitVersionDialog>,
    dialog: gpui_kit::component::dialog::Dialog,
    window: &Window,
    cx: &App,
) -> gpui_kit::component::dialog::Dialog {
    let state = this.read(cx);
    let Some(install) = state.installed(cx) else {
        return dialog;
    };
    let copy = GitVersionCopy::of(&install, state.unchanged);
    let palette = cx.palette();
    let bold: Vec<&str> = copy.bold.iter().map(String::as_str).collect();
    let explanation = with_bold(copy.explanation.clone(), &bold, cx);
    let title = copy.title;
    let still = copy.still;
    let checking = state.checking;
    let check = this.clone();
    dialog
        .w(CONFIRM_WIDTH.to_pixels(window.rem_size()))
        // Nothing works until git is updated, so there's nothing to go back to.
        .overlay_closable(false)
        .keyboard(false)
        .close_button(false)
        .child(
            DialogHeading::new(IconName::TriangleAlert, Tone::Coral, title)
                .description(div().child(explanation))
                .description(div().child(
                    "Once you've installed it, choose Check again. There's no need to restart \
                     Gitto.",
                ))
                .when_some(still, |heading, still| {
                    heading.description(
                        div()
                            .id("git-version-status")
                            .test_support()
                            .role(Role::Status)
                            .text_xs()
                            .text_color(palette.coral)
                            .child(still),
                    )
                }),
        )
        .footer(
            DialogFooter::new()
                .gap_2()
                .child(
                    Button::new("check-again")
                        .icon(Icon::new(IconName::RefreshCw))
                        .label(if checking {
                            "Checking…"
                        } else {
                            "Check again"
                        })
                        .disabled(checking)
                        .on_click(move |_, window, cx| {
                            check.update(cx, |this, cx| this.check_again(window, cx));
                        }),
                )
                .child(
                    Button::new("download-git")
                        .primary()
                        .icon(Icon::new(IconName::ExternalLink))
                        .label("Download Git")
                        .on_click(|_, _, cx| cx.open_url(GIT_DOWNLOAD_URL)),
                ),
        )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use futures::FutureExt as _;
    use gitto_git::features::status::schema::StatusReply;
    use gitto_git::input::RepositoryId;

    use gitto_git::{GitApi, GitRepos, GitVersion};
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window, div,
    };

    use super::*;
    use crate::test_support;
    use gitto_app_data::data::git::{GitTask, RepositoryGit};
    use gitto_app_data::data::{DataError, Part};

    /// A repository's git that counts how many times it's asked for the status, which it can't
    /// give.
    #[derive(Default)]
    struct Statuses(Cell<usize>);

    impl RepositoryGit for Statuses {
        fn status(&self, _: Option<String>) -> GitTask<StatusReply> {
            self.0.set(self.0.get() + 1);
            async { Err(DataError::msg("No git.")) }.boxed()
        }
    }

    /// What `git version` says, as a test sets it.
    type Installed = Arc<Mutex<Option<String>>>;

    /// A window with the dialog's controller, git saying it's `version`.
    struct Host {
        dialog: Entity<GitVersionDialog>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn open(
        version: Option<&str>,
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Installed,
        gpui_kit::AnyWindowHandle,
        Entity<Host>,
    ) {
        let installed: Installed = Arc::new(Mutex::new(
            version.map(|version| format!("git version {version}\n")),
        ));
        let dir = test_support::init(cx, |services| {
            let reader = Arc::clone(&installed);
            let version = GitVersion::with_reader(move || {
                let said = reader.lock().unwrap().clone();
                async move { said }
            });
            let git = GitApi::with(GitRepos::new(services.repositories().clone()), version);
            services.with_git(git)
        });
        let (window, host) = test_support::open(cx, |window, cx| Host {
            dialog: cx.new(|cx| GitVersionDialog::new(window, cx)),
        });
        cx.run_until_parked();
        (dir, installed, window, host)
    }

    fn has_dialog(window: gpui_kit::AnyWindowHandle, cx: &mut TestAppContext) -> bool {
        cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
            .unwrap()
    }

    #[gpui_kit::test]
    fn isnt_shown_with_a_supported_git(cx: &mut TestAppContext) {
        let (_dir, _, window, host) = open(Some("2.43.0"), cx);
        assert!(!has_dialog(window, cx));
        assert!(!host.read_with(cx, |host, cx| host.dialog.read(cx).is_open()));
    }

    #[gpui_kit::test]
    fn says_which_git_is_installed_and_which_is_needed_until_git_is_updated(
        cx: &mut TestAppContext,
    ) {
        let (_dir, installed, window, _) = open(Some("2.39.3"), cx);
        let id = RepositoryId::parse(uuid::Uuid::now_v7().to_string()).unwrap();
        let statuses = Rc::new(Statuses::default());
        let repository = cx.update(|cx| Repositories::insert(&id, statuses.clone(), cx));
        repository.update(cx, |repository, cx| repository.show(Part::Status, cx));
        cx.run_until_parked();
        assert!(has_dialog(window, cx));

        // It can't be dismissed: nothing works until git is updated.
        cx.update_window(window, |_, window, cx| window.press("escape", cx))
            .unwrap();
        assert!(has_dialog(window, cx));

        cx.update_window(window, |_, window, cx| window.click("download-git", cx))
            .unwrap();
        assert_eq!(cx.opened_url().as_deref(), Some(GIT_DOWNLOAD_URL));

        cx.update_window(window, |_, window, cx| window.click("check-again", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("git-version-status").is_some());
        })
        .unwrap();

        *installed.lock().unwrap() = Some("git version 2.51.0\n".into());
        cx.update_window(window, |_, window, cx| window.click("check-again", cx))
            .unwrap();
        cx.run_until_parked();
        assert!(!has_dialog(window, cx));
        // What failed meanwhile is loaded again: what an open repository shows of itself.
        assert_eq!(statuses.0.get(), 2);
        assert!(!repository.read_with(cx, |repository, _| repository.status().is_fetching()));
    }

    #[test]
    fn says_what_git_is_installed_and_needed() {
        let install = GitInstall {
            version: Some("2.39.3".into()),
            required: "2.41".into(),
            supported: false,
        };
        let copy = GitVersionCopy::of(&install, false);
        assert_eq!(copy.title, "Gitto needs a newer version of Git");
        assert!(
            copy.explanation
                .starts_with("Git 2.39.3 is installed, but Gitto needs Git 2.41 or newer.")
        );
        assert_eq!(copy.still, None);
        let still = GitVersionCopy::of(&install, true).still.unwrap();
        assert!(still.starts_with("Still finding Git 2.39.3."));
    }

    #[test]
    fn says_when_git_cant_be_found() {
        let install = GitInstall {
            version: None,
            required: "2.41".into(),
            supported: false,
        };
        let copy = GitVersionCopy::of(&install, true);
        assert_eq!(copy.title, "Gitto couldn't find Git");
        assert!(
            copy.explanation
                .ends_with("Install Git 2.41 or newer to get started.")
        );
        assert!(copy.still.unwrap().starts_with("Still no Git found."));
    }
}
