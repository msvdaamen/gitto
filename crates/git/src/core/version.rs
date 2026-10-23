//! Which git is installed, and whether Gitto works with it. Ported from
//! `packages/git/src/core/version.ts`.

use std::future::Future;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use futures::future::{BoxFuture, FutureExt, Shared};

use super::errors::{Error, Result};
use super::runner::lock;
use crate::schema::GitInstall;

/// The oldest git Gitto works with: it uses options and output that older ones don't have.
pub const MIN_GIT_VERSION: &str = "2.41";

/// The version in `git version`'s output, e.g. `2.45.1` from `git version 2.45.1.windows.1` or
/// `2.39.3` from `git version 2.39.3 (Apple Git-146)`; `None` if there's none.
pub fn parse_git_version(output: &str) -> Option<String> {
    let rest = output.trim().strip_prefix("git version ")?;
    // `\d+\.\d+(?:\.\d+)?`
    let mut end = 0;
    let mut parts = 0;
    let bytes = rest.as_bytes();
    while parts < 3 {
        let digits = bytes[end..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            break;
        }
        let after = end + digits;
        parts += 1;
        end = after;
        if parts == 3 || bytes.get(after) != Some(&b'.') {
            break;
        }
        if !bytes.get(after + 1).is_some_and(u8::is_ascii_digit) {
            break;
        }
        end = after + 1;
    }
    (parts >= 2).then(|| rest[..end].to_owned())
}

/// Whether `version` (e.g. `2.39.3`) is `MIN_GIT_VERSION` or newer.
pub fn is_supported_version(version: &str) -> bool {
    fn major_minor(version: &str) -> (u64, u64) {
        let mut parts = version.split('.').map(|part| part.parse().unwrap_or(0));
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    }
    let (major, minor) = major_minor(version);
    let (min_major, min_minor) = major_minor(MIN_GIT_VERSION);
    if major != min_major {
        major > min_major
    } else {
        minor >= min_minor
    }
}

/// Runs `git version`; resolves to its output, or `None` if git couldn't be run.
async fn read_git_version() -> Option<String> {
    let output = async_process::Command::new("git")
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
    /// be run. For tests.
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
    let required = MIN_GIT_VERSION.to_owned();
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
            supported: is_supported_version(&version),
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
        assert_eq!(parse("something else"), None);
    }

    #[test]
    fn is_supported_version_accepts_2_41_and_newer() {
        assert!(is_supported_version("2.41"));
        assert!(is_supported_version("2.41.0"));
        assert!(is_supported_version("2.51.2"));
        assert!(is_supported_version("3.0.0"));
        assert!(!is_supported_version("2.40.1"));
        assert!(!is_supported_version("2.39.5"));
        assert!(!is_supported_version("2.4.0"));
        assert!(!is_supported_version("1.99.0"));
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
