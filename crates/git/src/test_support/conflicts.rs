//! Repositories with conflicts, for the tests of resolving them and of what's under way. Ported
//! from `packages/git/src/test/conflicts.ts`.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Fixture, git};
use crate::core::repo::Repo;
use crate::features::conflicts::commands::{get_conflict, mark_resolved};
use crate::features::diff::working_tree::{SaveExpectation, save_working_tree_file};

/// Files to write, by path: their contents, or `None` to delete one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Files(pub Vec<(String, Option<Vec<u8>>)>);

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    /// With the file at `path` written as `contents` (text or bytes).
    pub fn with(mut self, path: &str, contents: impl AsRef<[u8]>) -> Self {
        self.0
            .push((path.to_owned(), Some(contents.as_ref().to_vec())));
        self
    }

    /// With the file at `path` deleted.
    pub fn without(mut self, path: &str) -> Self {
        self.0.push((path.to_owned(), None));
        self
    }
}

/// Text files to write, by path: `files([("f.txt", "one\n")])`.
pub fn files<const N: usize>(entries: [(&str, &str); N]) -> Files {
    entries
        .into_iter()
        .fold(Files::new(), |files, (path, contents)| {
            files.with(path, contents)
        })
}

/// What a file's base had, and what each side changed it to (see `create_diverged_repo`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sides {
    pub base: Files,
    pub ours: Files,
    pub theirs: Files,
}

/// `f.txt` with its second line changed on both sides.
pub fn text_conflict() -> Sides {
    Sides {
        base: files([("f.txt", "one\ntwo\nthree\n")]),
        ours: files([("f.txt", "one\nours\nthree\n")]),
        theirs: files([("f.txt", "one\ntheirs\nthree\n")]),
    }
}

/// Runs git in `cwd`, which may fail, as a merge that conflicts does; returns its exit code (`-1`
/// if it had none).
pub fn try_git<I, S>(cwd: impl AsRef<Path>, args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .args(args)
        .current_dir(cwd.as_ref())
        .output()
        .expect("git runs")
        .status
        .code()
        .unwrap_or(-1)
}

/// Writes `files` in the repository at `path`, and commits them with `message`.
pub fn commit_files(path: &Path, files: &Files, message: &str) {
    for (file, contents) in &files.0 {
        match contents {
            None => fs::remove_file(path.join(file)).unwrap(),
            Some(contents) => fs::write(path.join(file), contents).unwrap(),
        }
    }
    git(path, ["add", "-A"]);
    git(path, ["commit", "-qm", message]);
}

impl Fixture {
    /// A repository, which `repos` opens as `name`, where `main` and `side` both changed what
    /// `base` committed: `main` to `ours`, `side` to `theirs`. `main` is checked out, with
    /// `conflict_style` (`merge`, `diff3`, `zdiff3`) set if given. Returns its path.
    pub fn create_diverged_repo(
        &self,
        name: &str,
        sides: &Sides,
        conflict_style: Option<&str>,
    ) -> PathBuf {
        let path = self.create_repo(name);
        if let Some(style) = conflict_style {
            git(&path, ["config", "merge.conflictStyle", style]);
        }
        let mut base = files([("README", "readme\n")]);
        base.0.extend(sides.base.0.iter().cloned());
        commit_files(&path, &base, "base");
        git(&path, ["checkout", "-qb", "side"]);
        commit_files(&path, &sides.theirs, "theirs");
        git(&path, ["checkout", "-q", "main"]);
        commit_files(&path, &sides.ours, "ours");
        path
    }

    /// A repository merging `side` into `main`, stopped at the conflicts of `sides` (e.g.
    /// `text_conflict()`).
    pub fn create_merge_conflict(
        &self,
        name: &str,
        conflict_style: Option<&str>,
        sides: &Sides,
    ) -> PathBuf {
        let path = self.create_diverged_repo(name, sides, conflict_style);
        try_git(&path, ["merge", "-q", "side"]);
        path
    }
}

/// Resolves `f.txt`'s text conflict as `contents`, and marks it resolved, as the resolver does.
pub async fn resolve_text(repo: &Repo, contents: &str) {
    let text = get_conflict(repo, "f.txt").await.unwrap().text.unwrap();
    let expected = SaveExpectation {
        version: text.version,
        overwrite: false,
    };
    let version = save_working_tree_file(repo, "f.txt", contents, &expected)
        .await
        .unwrap();
    mark_resolved(repo, "f.txt", Some(&version), false)
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_conflict_is_left_in_the_index() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("conflicted", Some("diff3"), &text_conflict());
        assert_eq!(
            git(&path, ["diff", "--name-only", "--diff-filter=U"]),
            "f.txt"
        );
        let text = fs::read_to_string(path.join("f.txt")).unwrap();
        assert!(text.contains("<<<<<<< HEAD\nours\n||||||| "), "{text}");
    }
}
