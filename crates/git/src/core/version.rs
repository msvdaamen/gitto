//! Which git is installed, and whether Gitto works with it. Ported from
//! `packages/git/src/core/version.ts`.

use std::fmt;
use std::future::Future;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex};

use futures::future::{BoxFuture, FutureExt, Shared};
use regex::Regex;

use super::errors::{Error, Result};
use super::runner::lock;
use super::shell_path::git_path;
use crate::schema::GitInstall;

/// A git version, e.g. `2.39.3`, ordered as versions are; `2.41` is `2.41.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version {
            major,
            minor,
            patch,
        }
    }
}

impl FromStr for Version {
    type Err = Error;

    /// `2.39.3`, or `2.41`; fails with `Error::InvalidInput` on anything else.
    fn from_str(text: &str) -> Result<Self> {
        // `[0-9]` rather than `\d`, which is any Unicode digit here, and only ASCII ones in JS.
        static VERSION: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^([0-9]+)\.([0-9]+)(?:\.([0-9]+))?$").expect("a valid regex")
        });
        let invalid = || Error::InvalidInput(format!("Not a git version. ({text:?})"));
        let captures = VERSION.captures(text).ok_or_else(invalid)?;
        let part = |index| match captures.get(index) {
            Some(part) => part.as_str().parse().map_err(|_| invalid()),
            None => Ok(0),
        };
        Ok(Version::new(part(1)?, part(2)?, part(3)?))
    }
}

impl fmt::Display for Version {
    /// `2.41.1`, or `2.41` for `2.41.0`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        if self.patch != 0 {
            write!(f, ".{}", self.patch)?;
        }
        Ok(())
    }
}

/// The oldest git Gitto works with: it uses options and output that older ones don't have.
pub(crate) const MIN_GIT_VERSION: Version = Version::new(2, 41, 0);

/// The version in `git version`'s output, as git words it, e.g. `2.45.1` from
/// `git version 2.45.1.windows.1` or `2.39.3` from `git version 2.39.3 (Apple Git-146)`; `None`
/// if there's none.
pub(crate) fn parse_git_version(output: &str) -> Option<String> {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^git version ([0-9]+\.[0-9]+(?:\.[0-9]+)?)").expect("a valid regex")
    });
    let captures = VERSION.captures(output.trim())?;
    Some(captures[1].to_owned())
}

/// Runs `git version`; resolves to its output, or `None` if git couldn't be run.
async fn read_git_version() -> Option<String> {
    let mut command = async_process::Command::new("git");
    // The git that runs the commands (see `shell_path`).
    if let Some(path) = git_path().await {
        command.env("PATH", path);
    }
    let output = command
        .arg("version")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

type Read = Arc<dyn Fn() -> BoxFuture<'static, Option<String>> + Send + Sync>;

/// Checks that the installed git is one Gitto works with. A supported git is only checked once per
/// run of the app; one that isn't is checked again every time, so Gitto works again as soon as the
/// user has updated git, without restarting.
pub struct GitVersion {
    read: Read,
    checked: Mutex<Option<Shared<BoxFuture<'static, GitInstall>>>>,
}

impl Default for GitVersion {
    fn default() -> Self {
        Self::new()
    }
}

impl GitVersion {
    /// Checks the git on the PATH.
    pub fn new() -> Self {
        Self::with_reader(|| read_git_version().boxed())
    }

    /// Checks the git `read` says is installed: its `git version` output, or `None` if it couldn't
    /// be run. E.g. a git of another version than the one installed, in the app's tests.
    pub fn with_reader<F, Fut>(read: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<String>> + Send + 'static,
    {
        GitVersion {
            read: Arc::new(move || read().boxed()),
            checked: Mutex::new(None),
        }
    }

    /// The installed git, and whether Gitto works with it. Until it does, every procedure on a
    /// repository fails saying so (see `require`). Checked again on every call until it does,
    /// e.g. after the user updated git. TS's `version` feature's procedure.
    pub async fn check(&self) -> GitInstall {
        let checked = lock(&self.checked)
            .get_or_insert_with(|| (self.read)().map(to_install).boxed().shared())
            .clone();
        let install = checked.clone().await;
        // Only a supported git is kept; the next call checks again.
        if !install.supported {
            let mut kept = lock(&self.checked);
            if kept.as_ref().is_some_and(|kept| kept.ptr_eq(&checked)) {
                *kept = None;
            }
        }
        install
    }

    /// Fails with `Error::UnsupportedGit` if the installed git isn't supported.
    pub async fn require(&self) -> Result<()> {
        let install = self.check().await;
        if install.supported {
            Ok(())
        } else {
            Err(Error::UnsupportedGit {
                version: install.version,
                required: install.required,
            })
        }
    }
}

fn to_install(output: Option<String>) -> GitInstall {
    let required = MIN_GIT_VERSION.to_string();
    let Some(output) = output else {
        return GitInstall {
            version: None,
            required,
            supported: false,
        };
    };
    match parse_git_version(&output) {
        // A build that words it differently is given the benefit of the doubt.
        None => GitInstall {
            version: Some(output.trim().to_owned()),
            required,
            supported: true,
        },
        Some(version) => GitInstall {
            // It parses: it's what `parse_git_version` matched.
            supported: version
                .parse::<Version>()
                .is_ok_and(|version| version >= MIN_GIT_VERSION),
            version: Some(version),
            required,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn parse_git_version_reads_the_version_from_what_each_platforms_git_prints() {
        let parse = |output| parse_git_version(output);
        assert_eq!(parse("git version 2.43.0\n").as_deref(), Some("2.43.0"));
        assert_eq!(
            parse("git version 2.45.1.windows.1\n").as_deref(),
            Some("2.45.1")
        );
        assert_eq!(
            parse("git version 2.39.3 (Apple Git-146)\n").as_deref(),
            Some("2.39.3")
        );
        assert_eq!(parse("git version 2.50.0.rc1\n").as_deref(), Some("2.50.0"));
        assert_eq!(parse("git version 2.50\n").as_deref(), Some("2.50"));
        assert_eq!(parse("git version 2.\n"), None);
        // Only ASCII digits, as JS's `\d` has it.
        assert_eq!(parse("git version \u{0662}.\u{0664}\n"), None);
        assert_eq!(parse("something else"), None);
    }

    #[test]
    fn versions_are_ordered_as_versions_are() {
        let version = |text: &str| text.parse::<Version>().unwrap();
        assert_eq!(version("2.41"), Version::new(2, 41, 0));
        assert_eq!(version("2.39.3"), Version::new(2, 39, 3));
        for supported in ["2.41", "2.41.0", "2.51.2", "3.0.0"] {
            assert!(version(supported) >= MIN_GIT_VERSION, "{supported}");
        }
        for unsupported in ["2.40.1", "2.39.5", "2.4.0", "1.99.0"] {
            assert!(version(unsupported) < MIN_GIT_VERSION, "{unsupported}");
        }
        assert_eq!(MIN_GIT_VERSION.to_string(), "2.41");
        assert_eq!(Version::new(2, 41, 1).to_string(), "2.41.1");
        assert!("2".parse::<Version>().is_err());
        assert!("2.x".parse::<Version>().is_err());
        assert!("\u{0662}.\u{0664}".parse::<Version>().is_err());
    }

    #[test]
    fn checks_a_supported_git_only_once() {
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let version = GitVersion::with_reader(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            async { Some("git version 2.43.0\n".to_owned()) }
        });
        smol::block_on(async {
            version.check().await;
            version.require().await.unwrap();
        });
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn checks_an_unsupported_git_again_until_its_been_updated() {
        let outputs = [
            Some("git version 2.39.3\n"),
            None,
            Some("git version 2.41.1\n"),
        ];
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&reads);
        let version = GitVersion::with_reader(move || {
            let read = counted.fetch_add(1, Ordering::SeqCst);
            let output = outputs.get(read).copied().flatten().map(str::to_owned);
            async move { output }
        });
        smol::block_on(async {
            let install = version.check().await;
            assert_eq!(install.version.as_deref(), Some("2.39.3"));
            assert!(!install.supported);
            let error = version.require().await.unwrap_err();
            assert!(
                error.to_string().contains("Gitto couldn't find Git."),
                "{error}"
            );
            let install = version.check().await;
            assert_eq!(install.version.as_deref(), Some("2.41.1"));
            assert!(install.supported);
            version.require().await.unwrap();
        });
        assert_eq!(reads.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn doesnt_refuse_a_git_whose_version_it_cant_read() {
        let version =
            GitVersion::with_reader(|| async { Some("git version unknown\n".to_owned()) });
        assert!(smol::block_on(version.check()).supported);
    }

    #[test]
    fn runs_the_installed_git() {
        let install = smol::block_on(GitVersion::new().check());
        let version = install.version.unwrap();
        assert!(
            version
                .split('.')
                .take(2)
                .all(|part| part.parse::<u32>().is_ok()),
            "{version}"
        );
        assert_eq!(install.required, "2.41");
    }
}
