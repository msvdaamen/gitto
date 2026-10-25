//! Where a repository keeps what git knows of it. Ported from `packages/git/src/core/git-dirs.ts`.

use std::io;
use std::path::{Path, PathBuf};

use super::errors::Result;
use super::repo::Repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDirs {
    /// This worktree's git directory: HEAD, the index.
    pub git_dir: PathBuf,
    /// The git directory shared by all worktrees: refs, config. The same as `git_dir`, usually.
    pub common_dir: PathBuf,
    pub exclude_file: PathBuf,
}

/// The repository's git directories, symlinks resolved (that's how the watcher reports paths).
pub async fn git_dirs(repo: &Repo) -> Result<GitDirs> {
    let output = repo
        .read([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
            "--git-path",
            "info/exclude",
        ])
        .await?;
    let mut lines = output.split('\n');
    let mut next = || lines.next().unwrap_or_default().to_owned();
    let (git_dir, common_dir, exclude_file) = (next(), next(), next());
    let (git_dir, common_dir) = futures::try_join!(real_path(&git_dir), real_path(&common_dir))?;
    Ok(GitDirs {
        git_dir,
        common_dir,
        exclude_file: PathBuf::from(exclude_file),
    })
}

/// `path` with symlinks resolved, as Node's `realpath` has it (not as a `\\?\` path on Windows).
/// A failure names `path`, as Node's does: the OS's message alone doesn't say which one it was.
pub async fn real_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let real = async_fs::canonicalize(path).await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{error}, realpath '{}'", path.display()),
        )
    })?;
    Ok(dunce::simplified(&real).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Fixture, git};

    #[test]
    fn are_the_worktrees_own_and_the_shared_one() {
        let fx = Fixture::new();
        let path = fx.create_repo("dirs");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        let linked = fx.root().join("dirs-linked");
        git(
            &path,
            [
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        fx.add_path("dirs-linked", &linked);
        smol::block_on(async {
            let main = git_dirs(&fx.repos.open("dirs").await.unwrap())
                .await
                .unwrap();
            assert_eq!(main.git_dir, path.join(".git"));
            assert_eq!(main.common_dir, path.join(".git"));
            assert_eq!(main.exclude_file, path.join(".git/info/exclude"));

            let other = git_dirs(&fx.repos.open("dirs-linked").await.unwrap())
                .await
                .unwrap();
            assert_eq!(other.git_dir, path.join(".git/worktrees/dirs-linked"));
            assert_eq!(other.common_dir, path.join(".git"));
        });
    }

    #[test]
    fn real_path_names_the_path_it_couldnt_resolve() {
        let fx = Fixture::new();
        let missing = fx.root().join("missing");
        let error = smol::block_on(real_path(&missing)).unwrap_err();
        assert!(matches!(error, crate::Error::Io(_)), "{error:?}");
        assert!(
            error
                .to_string()
                .ends_with(&format!(", realpath '{}'", missing.display())),
            "{error}"
        );
    }
}
