//! The installed git, and whether Gitto works with it (see `crate::core::version`).
//!
//! Ported from `packages/git/src/features/version/`, which only had the procedure: it's
//! [`GitVersion::check`], which this hands on.

use crate::core::version::GitVersion;
use crate::schema::GitInstall;

/// The installed git, and whether Gitto works with it. Until it does, every procedure on a
/// repository fails saying so. Checked again on every call until it does, e.g. after the user
/// updated git.
pub async fn check(git_version: &GitVersion) -> GitInstall {
    git_version.check().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_the_installed_git() {
        let version = GitVersion::with_reader(|| async { Some("git version 2.39.3\n".to_owned()) });
        let install = smol::block_on(check(&version));
        assert_eq!(
            install,
            GitInstall {
                version: Some("2.39.3".into()),
                required: "2.41".into(),
                supported: false,
            }
        );
    }
}
