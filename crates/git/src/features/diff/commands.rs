//! Reading diffs: the files a commit changed, one file's patch from a commit, the index or the
//! working tree, and a file's contents by its object name. Ported from `commands.ts`.
//!
//! TS's `PatchOptions.binary` is the `*_bytes` variant of each patch function, which reads the
//! patch as bytes rather than as UTF-8; its `signal` is dropping the future.

use futures::future::BoxFuture;

use super::limits::{MAX_BLOB_BYTES, MAX_PATCH_BYTES};
use super::parse::parse_diff;
use super::schema::ChangedFile;
use super::working_tree::check_working_tree_path;
use crate::core::errors::{Error, GitError, Result};
use crate::core::repo::{GitCommand, Repo};
use crate::core::runner::RunOptions;
use crate::input::FileInput;

/// The files the commit `sha` changed compared to its first parent, with their line counts.
pub async fn get_commit_files(repo: &Repo, sha: &str) -> Result<Vec<ChangedFile>> {
    let output = repo
        .read([
            "diff-tree",
            "-r",
            "--root",
            "-M",
            "--raw",
            "--numstat",
            "-z",
            "--no-commit-id",
            "--diff-merges=first-parent",
            sha,
        ])
        .await?;
    Ok(parse_diff(&output))
}

/// The patch of one file a commit changed, compared to its first parent like `get_commit_files`,
/// so its lines are the ones counted there. A renamed file needs its old path too, to be found as
/// one. With full object names on its `index` line, which `get_blob` reads the whole file by.
pub fn get_commit_file_patch(
    repo: &Repo,
    sha: &str,
    file: &FileInput,
) -> BoxFuture<'static, Result<String>> {
    let mut args = vec![
        "diff-tree",
        "-p",
        "-r",
        "--root",
        "-M",
        "--full-index",
        "--no-commit-id",
        "--diff-merges=first-parent",
        sha,
        "--",
    ];
    args.extend(file_paths(file));
    read_patch(&repo.git(), args)
}

/// The patch of one file's unstaged changes: the working tree compared to the index, as in the
/// list of unstaged files. The `index` line names the working tree's side by what git would store
/// it as, which isn't in the repository: `read_working_tree_file` reads it whole. An untracked file
/// is shown as added, in full.
pub async fn get_unstaged_file_patch(
    repo: &Repo,
    file: &FileInput,
    untracked: bool,
) -> Result<String> {
    unstaged_file_patch(repo, file, untracked).await
}

/// `get_unstaged_file_patch` as bytes rather than as UTF-8: to hand a patch of a file that isn't
/// UTF-8 back to git as it came.
pub async fn get_unstaged_file_patch_bytes(
    repo: &Repo,
    file: &FileInput,
    untracked: bool,
) -> Result<Vec<u8>> {
    unstaged_file_patch(repo, file, untracked).await
}

async fn unstaged_file_patch<T: Patch>(
    repo: &Repo,
    file: &FileInput,
    untracked: bool,
) -> Result<T> {
    if untracked {
        return untracked_file_patch(repo, &file.path).await;
    }
    let mut args = vec!["diff-files", "-p", "-M", "--full-index", "--"];
    args.extend(file_paths(file));
    T::read(&repo.git(), args).await
}

/// An untracked file, compared to nothing. Git only compares files outside the index with
/// porcelain `diff --no-index`, which reads the user's diff settings (see `PORCELAIN_DIFF`). It
/// reads any path it's given, so only one inside the working tree is: a link there is read as
/// where it points.
async fn untracked_file_patch<T: Patch>(repo: &Repo, path: &str) -> Result<T> {
    check_working_tree_path(repo, path).await?;
    let mut args = vec!["diff", "--no-index", "--full-index"];
    args.extend(PORCELAIN_DIFF);
    args.extend(["--", "/dev/null", path]);
    match T::read(&repo.git(), args).await {
        // Exits with 1 when the files differ, which they always do, but also when it can't read
        // one, like a folder: the patch tells them apart.
        Err(error) if error.is_exit(1) => match error.as_git() {
            Some(git) if !git.stdout.is_empty() => Ok(T::from_stdout(git)),
            _ => Err(error),
        },
        result => result,
    }
}

/// The patch of one file's staged changes: the index compared to HEAD, as in the list of staged
/// files, or to nothing before the first commit. Both sides are in the repository, by the object
/// names on the `index` line.
pub async fn get_staged_file_patch(repo: &Repo, file: &FileInput) -> Result<String> {
    staged_file_patch(repo, file).await
}

/// `get_staged_file_patch` as bytes rather than as UTF-8: to hand a patch of a file that isn't
/// UTF-8 back to git as it came.
pub async fn get_staged_file_patch_bytes(repo: &Repo, file: &FileInput) -> Result<Vec<u8>> {
    staged_file_patch(repo, file).await
}

async fn staged_file_patch<T: Patch>(repo: &Repo, file: &FileInput) -> Result<T> {
    let run = repo.git();
    let diff = |base: &str| {
        let mut args = vec![
            "diff-index",
            "-p",
            "--cached",
            "-M",
            "--full-index",
            base,
            "--",
        ];
        args.extend(file_paths(file));
        T::read(&run, args)
    };
    match diff("HEAD").await {
        Ok(patch) => Ok(patch),
        Err(error) => {
            // Asked only once it failed: there's a HEAD all but before the first commit.
            if error.as_git().is_none() || repo.has_head().await {
                return Err(error);
            }
            let tree = empty_tree(&run).await?;
            diff(&tree).await
        }
    }
}

/// The object name of a tree without files, in the repository's hash.
async fn empty_tree(run: &GitCommand) -> Result<String> {
    let output = run
        .run_with(
            ["hash-object", "-t", "tree", "--stdin"],
            RunOptions::default().stdin(""),
        )
        .await?;
    Ok(output.trim().to_owned())
}

/// A file's path, and its previous one if it was renamed: a rename is only found with both.
fn file_paths(file: &FileInput) -> Vec<&str> {
    match file.orig_path.as_deref() {
        Some(orig_path) if !orig_path.is_empty() => vec![&file.path, orig_path],
        _ => vec![&file.path],
    }
}

/// A patch as it's read: as text, or as bytes (TS's `binary`).
trait Patch: Sized + Send + 'static {
    fn read(run: &GitCommand, args: Vec<&str>) -> BoxFuture<'static, Result<Self>>;
    /// The patch a failed command printed (see `GitError::stdout`).
    fn from_stdout(error: &GitError) -> Self;
}

impl Patch for String {
    fn read(run: &GitCommand, args: Vec<&str>) -> BoxFuture<'static, Result<Self>> {
        read_patch(run, args)
    }

    fn from_stdout(error: &GitError) -> Self {
        error.stdout_text().into_owned()
    }
}

impl Patch for Vec<u8> {
    fn read(run: &GitCommand, args: Vec<&str>) -> BoxFuture<'static, Result<Self>> {
        read_patch_bytes(run, args)
    }

    /// As git printed it: a file that isn't UTF-8 keeps the bytes that aren't, as in TS, which
    /// read it as Latin-1.
    fn from_stdout(error: &GitError) -> Self {
        error.stdout.clone()
    }
}

/// How a patch is read: stopped past `MAX_PATCH_BYTES`.
fn patch_options() -> RunOptions {
    RunOptions::default().max_output(
        MAX_PATCH_BYTES as usize,
        Error::ChangesTooLarge {
            bytes: MAX_PATCH_BYTES,
        },
    )
}

/// Runs a diff command for one file's patch, through `run`; stopped past `MAX_PATCH_BYTES`.
pub fn read_patch<I, S>(run: &GitCommand, args: I) -> BoxFuture<'static, Result<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    run.run_with(args, patch_options())
}

/// `read_patch`, as bytes rather than as UTF-8 (TS's `binary`).
pub fn read_patch_bytes<I, S>(run: &GitCommand, args: I) -> BoxFuture<'static, Result<Vec<u8>>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    run.run_bytes_with(args, patch_options())
}

/// Undoes the user's diff settings that would change a patch from a porcelain command, which reads
/// them (plumbing doesn't): other prefixes than `a/` and `b/`, an external diff tool, a textconv
/// filter. `--default-prefix` rather than `--src-prefix` and `--dst-prefix`, which `git stash show`
/// mangles.
pub const PORCELAIN_DIFF: [&str; 4] = [
    "--default-prefix",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
];

/// The contents of a file, by its object name; fails with `Error::FileTooLarge` above
/// `MAX_BLOB_BYTES`.
pub async fn get_blob(repo: &Repo, oid: &str) -> Result<String> {
    let size = repo.read(["cat-file", "-s", oid]).await?;
    // A size that isn't a number (it always is) is no reason not to read it, as in TS (`NaN`).
    let bytes = size.trim().parse::<u64>().unwrap_or(0);
    if bytes > MAX_BLOB_BYTES {
        return Err(Error::FileTooLarge { bytes });
    }
    repo.read(["cat-file", "blob", oid]).await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use regex::Regex;

    use super::*;
    use crate::core::errors::{DiskChange, GitErrorKind};
    use crate::features::diff::working_tree::{
        SaveExpectation, read_working_tree_file, save_working_tree_file,
    };
    use crate::schema::FileStatus;
    use crate::test_support::{Fixture, git};

    fn file(path: &str, orig_path: Option<&str>) -> FileInput {
        FileInput {
            path: path.to_owned(),
            orig_path: orig_path.map(str::to_owned),
        }
    }

    fn changed(
        path: &str,
        status: FileStatus,
        orig_path: Option<&str>,
        additions: Option<u64>,
        deletions: Option<u64>,
    ) -> ChangedFile {
        ChangedFile {
            path: path.to_owned(),
            status,
            orig_path: orig_path.map(str::to_owned),
            additions,
            deletions,
        }
    }

    /// The commit with `subject` on `main`. TS found it in history's `getLog`, which isn't a
    /// dependency of this feature; the log git prints has the same commits.
    fn sha_of(path: &Path, subject: &str) -> String {
        git(path, ["log", "--format=%H %s", "main"])
            .lines()
            .find_map(|line| {
                let (sha, line_subject) = line.split_once(' ')?;
                (line_subject == subject).then(|| sha.to_owned())
            })
            .unwrap_or_else(|| panic!("no commit {subject:?}"))
    }

    fn is_git_error(error: &Error) -> bool {
        matches!(error, Error::Git(git) if git.kind == GitErrorKind::Command)
    }

    fn is_outside(error: &Error) -> bool {
        matches!(error, Error::OutsideRepository { .. })
    }

    fn is_changed_on_disk(error: &Error) -> bool {
        matches!(error, Error::FileChangedOnDisk { .. })
    }

    fn expected(version: &str, overwrite: bool) -> SaveExpectation {
        SaveExpectation {
            version: version.to_owned(),
            overwrite,
        }
    }

    /// The permissions bits of the file at `path`.
    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    fn symlink(original: impl AsRef<Path>, link: impl AsRef<Path>) {
        std::os::unix::fs::symlink(original, link).unwrap();
    }

    // getCommitFiles

    #[test]
    fn diffs_a_merge_against_its_first_parent_and_a_root_commit_against_nothing() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let merge = sha_of(repo.path(), "Merge side");
            let first = sha_of(repo.path(), "first");
            assert_eq!(
                get_commit_files(&repo, &merge).await.unwrap(),
                vec![
                    changed(
                        "c.txt",
                        FileStatus::Renamed,
                        Some("b.txt"),
                        Some(0),
                        Some(0)
                    ),
                    changed("s.txt", FileStatus::Added, None, Some(1), Some(0)),
                ]
            );
            assert_eq!(
                get_commit_files(&repo, &first).await.unwrap(),
                vec![
                    changed("a file.txt", FileStatus::Added, None, Some(1), Some(0)),
                    changed("b.txt", FileStatus::Added, None, Some(1), Some(0)),
                    changed("bin.dat", FileStatus::Added, None, None, None),
                ]
            );
        });
    }

    #[test]
    fn explains_why_a_command_failed() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let error = get_commit_files(&repo, "0123456789abcdef0123456789abcdef01234567")
                .await
                .unwrap_err();
            assert!(is_git_error(&error), "{error:?}");
            assert!(error.to_string().contains("bad object"), "{error}");
        });
    }

    // getCommitFilePatch

    #[test]
    fn shows_one_files_changes_with_its_full_object_names() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("patches").await;
            let sha = sha_of(repo.path(), "main commit");
            let patch = get_commit_file_patch(&repo, &sha, &file("a file.txt", None))
                .await
                .unwrap();
            let header = Regex::new(
                r"^diff --git a/a file.txt b/a file.txt\nindex [0-9a-f]{40}\.\.[0-9a-f]{40} 100644\n",
            )
            .unwrap();
            assert!(header.is_match(&patch), "{patch}");
            assert!(patch.contains("@@ -1 +1,2 @@\n a\n+more\n"), "{patch}");
        });
    }

    #[test]
    fn finds_a_rename_with_its_old_path_compared_to_a_merges_first_parent() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("patches").await;
            let sha = sha_of(repo.path(), "Merge side");
            let patch = get_commit_file_patch(&repo, &sha, &file("c.txt", Some("b.txt")))
                .await
                .unwrap();
            assert!(
                patch.contains("rename from b.txt\nrename to c.txt\n"),
                "{patch}"
            );
        });
    }

    #[test]
    fn shows_a_root_commits_files_as_added() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("patches").await;
            let sha = sha_of(repo.path(), "first");
            let patch = get_commit_file_patch(&repo, &sha, &file("b.txt", None))
                .await
                .unwrap();
            assert!(patch.contains("new file mode 100644\n"), "{patch}");
            assert!(patch.contains("@@ -0,0 +1 @@\n+b\n"), "{patch}");
        });
    }

    // getBlob

    #[test]
    fn reads_a_file_by_the_object_name_on_a_patchs_index_line() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("blobs").await;
            let sha = sha_of(repo.path(), "main commit");
            let patch = get_commit_file_patch(&repo, &sha, &file("a file.txt", None))
                .await
                .unwrap();
            let index = Regex::new(r"(?m)^index ([0-9a-f]+)\.\.([0-9a-f]+)").unwrap();
            let captures = index.captures(&patch).unwrap();
            assert_eq!(get_blob(&repo, &captures[1]).await.unwrap(), "a\n");
            assert_eq!(get_blob(&repo, &captures[2]).await.unwrap(), "a\nmore\n");
        });
    }

    #[test]
    fn wont_read_a_file_too_large_to_send_whole() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("large-blob").await;
            let mut child = Command::new("git")
                .args(["hash-object", "-w", "--stdin"])
                .current_dir(repo.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let input = "x".repeat(MAX_BLOB_BYTES as usize + 1);
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            let oid = String::from_utf8(output.stdout).unwrap().trim().to_owned();
            let error = get_blob(&repo, &oid).await.unwrap_err();
            assert!(matches!(error, Error::FileTooLarge { .. }), "{error:?}");
        });
    }

    // getUnstagedFilePatch

    #[test]
    fn compares_the_working_tree_with_the_index_ignoring_the_users_diff_settings() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("unstaged").await;
            let path = repo.path();
            git(path, ["config", "diff.noprefix", "true"]);
            git(path, ["config", "diff.external", "false"]);
            git(path, ["config", "color.diff", "always"]);
            let patch = get_unstaged_file_patch(&repo, &file("a file.txt", None), false)
                .await
                .unwrap();
            let header = Regex::new(
                r"^diff --git a/a file.txt b/a file.txt\nindex [0-9a-f]{40}\.\.[0-9a-f]{40} 100644\n",
            )
            .unwrap();
            assert!(header.is_match(&patch), "{patch}");
            assert!(
                patch.contains("@@ -1,2 +1 @@\n-a\n-more\n+changed\n"),
                "{patch}"
            );
        });
    }

    #[test]
    fn shows_an_untracked_file_as_added_in_full() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("untracked").await;
            let path = repo.path();
            git(path, ["config", "diff.noprefix", "true"]);
            git(path, ["config", "diff.mnemonicPrefix", "true"]);
            let patch = get_unstaged_file_patch(&repo, &file("new file.txt", None), true)
                .await
                .unwrap();
            let header =
                Regex::new(r"^diff --git a/new file.txt b/new file.txt\nnew file mode 100644\n")
                    .unwrap();
            assert!(header.is_match(&patch), "{patch}");
            // A name with a space ends in a tab, as in other diffs.
            assert!(
                patch.contains("--- /dev/null\n+++ b/new file.txt\t\n@@ -0,0 +1 @@\n+new\n"),
                "{patch}"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn wont_compare_a_path_outside_the_working_tree_but_shows_a_link_out_as_one() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("untracked-confined").await;
            let path = repo.path();
            fs::write(path.join("..").join("outside.txt"), "secret\n").unwrap();
            symlink(path.join("..").join("outside.txt"), path.join("link"));
            let outside = ["../outside.txt", "/etc/hosts", ".git/config", ".GIT/config"];
            let files = outside.map(|name| file(name, None));
            let errors = futures::future::join_all(
                files
                    .iter()
                    .map(|file| get_unstaged_file_patch(&repo, file, true)),
            )
            .await;
            for error in errors {
                let error = error.unwrap_err();
                assert!(is_outside(&error), "{error:?}");
            }
            let link = get_unstaged_file_patch(&repo, &file("link", None), true)
                .await
                .unwrap();
            assert!(link.contains("new file mode 120000\n"), "{link}");
            assert!(!link.contains("secret"), "{link}");
        });
    }

    #[test]
    fn says_why_an_untracked_path_cant_be_compared_like_another_repositorys_folder() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("nested").await;
            fs::create_dir(repo.path().join("inner")).unwrap();
            let error = get_unstaged_file_patch(&repo, &file("inner/", None), true)
                .await
                .unwrap_err();
            assert!(is_git_error(&error), "{error:?}");
        });
    }

    #[test]
    fn is_empty_for_a_file_without_unstaged_changes() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("clean").await;
            assert_eq!(
                get_unstaged_file_patch(&repo, &file("b.txt", None), false)
                    .await
                    .unwrap(),
                ""
            );
        });
    }

    #[test]
    fn refuses_changes_too_large_to_send() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("huge").await;
            fs::write(
                repo.path().join("huge.log"),
                "line\n".repeat(MAX_PATCH_BYTES as usize / 4),
            )
            .unwrap();
            let error = get_unstaged_file_patch(&repo, &file("huge.log", None), true)
                .await
                .unwrap_err();
            assert!(matches!(error, Error::ChangesTooLarge { .. }), "{error:?}");
        });
    }

    // getStagedFilePatch

    #[test]
    fn compares_the_index_with_head_finding_renames() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("staged").await;
            let path = repo.path();
            git(path, ["add", "a file.txt"]);
            git(path, ["mv", "c.txt", "d.txt"]);
            let patch = get_staged_file_patch(&repo, &file("a file.txt", None))
                .await
                .unwrap();
            assert!(
                patch.contains("@@ -1,2 +1 @@\n-a\n-more\n+changed\n"),
                "{patch}"
            );
            let patch = get_staged_file_patch(&repo, &file("d.txt", Some("c.txt")))
                .await
                .unwrap();
            assert!(
                patch.contains("rename from c.txt\nrename to d.txt\n"),
                "{patch}"
            );
            assert_eq!(
                get_staged_file_patch(&repo, &file("s.txt", None))
                    .await
                    .unwrap(),
                ""
            );
        });
    }

    #[test]
    fn compares_with_nothing_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("unborn");
        smol::block_on(async {
            let repo = fx.repos.open("unborn").await.unwrap();
            fs::write(path.join("first.txt"), "first\n").unwrap();
            git(&path, ["add", "first.txt"]);
            let patch = get_staged_file_patch(&repo, &file("first.txt", None))
                .await
                .unwrap();
            assert!(patch.contains("new file mode 100644\n"), "{patch}");
            assert!(patch.contains("@@ -0,0 +1 @@\n+first\n"), "{patch}");
        });
    }

    // readWorkingTreeFile

    #[test]
    fn reads_a_file_in_the_working_tree_as_it_is_on_disk() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("read").await;
            fs::write(repo.path().join("crlf.txt"), "\u{FEFF}one\r\ntwo\r\n").unwrap();
            assert_eq!(
                read_working_tree_file(&repo, "a file.txt")
                    .await
                    .unwrap()
                    .contents,
                "changed\n"
            );
            assert_eq!(
                read_working_tree_file(&repo, "crlf.txt")
                    .await
                    .unwrap()
                    .contents,
                "\u{FEFF}one\r\ntwo\r\n"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn wont_read_outside_the_working_tree_or_in_the_git_directory() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("confined").await;
            let path = repo.path();
            symlink(path.join(".."), path.join("up"));
            symlink(path.join("c.txt"), path.join("inside"));
            fs::write(path.join("..").join("outside.txt"), "secret\n").unwrap();
            let outside = [
                "../outside.txt",
                "up/outside.txt",
                ".git/config",
                "/etc/hosts",
            ];
            let errors =
                futures::future::join_all(outside.map(|name| read_working_tree_file(&repo, name)))
                    .await;
            for error in errors {
                let error = error.unwrap_err();
                assert!(is_outside(&error), "{error:?}");
            }
            assert_eq!(
                read_working_tree_file(&repo, "inside")
                    .await
                    .unwrap()
                    .contents,
                "b\n"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn wont_read_the_git_directory_through_a_link_or_in_another_case() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("git-dir").await;
            symlink(".git", repo.path().join("meta"));
            let errors = futures::future::join_all(
                ["meta/config", ".GIT/config"].map(|name| read_working_tree_file(&repo, name)),
            )
            .await;
            for error in errors {
                let error = error.unwrap_err();
                assert!(is_outside(&error), "{error:?}");
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn says_when_a_file_is_gone_and_wont_wait_on_a_named_pipe() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("not-files").await;
            let status = Command::new("mkfifo")
                .arg(repo.path().join("pipe"))
                .status()
                .unwrap();
            assert!(status.success());
            let error = read_working_tree_file(&repo, "gone.txt").await.unwrap_err();
            assert!(
                matches!(error, Error::WorkingTreeFileNotFound { .. }),
                "{error:?}"
            );
            let error = read_working_tree_file(&repo, "pipe").await.unwrap_err();
            assert!(is_outside(&error), "{error:?}");
        });
    }

    #[test]
    fn wont_read_a_file_thats_too_large_or_isnt_utf_8() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("unreadable").await;
            let path = repo.path();
            fs::write(
                path.join("large.txt"),
                "x".repeat(MAX_BLOB_BYTES as usize + 1),
            )
            .unwrap();
            fs::write(path.join("latin1.txt"), [0x63, 0x61, 0x66, 0xe9]).unwrap();
            let error = read_working_tree_file(&repo, "large.txt")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::FileTooLarge { .. }), "{error:?}");
            let error = read_working_tree_file(&repo, "latin1.txt")
                .await
                .unwrap_err();
            assert!(matches!(error, Error::NotUtf8), "{error:?}");
        });
    }

    // saveWorkingTreeFile

    #[cfg(unix)]
    #[test]
    fn saves_edits_over_the_file_as_it_was_read_keeping_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("save").await;
            let path = repo.path();
            fs::set_permissions(path.join("a file.txt"), fs::Permissions::from_mode(0o755))
                .unwrap();
            let version = read_working_tree_file(&repo, "a file.txt")
                .await
                .unwrap()
                .version;

            let saved =
                save_working_tree_file(&repo, "a file.txt", "edited\n", &expected(&version, false))
                    .await
                    .unwrap();
            assert_eq!(
                fs::read_to_string(path.join("a file.txt")).unwrap(),
                "edited\n"
            );
            assert_eq!(mode(&path.join("a file.txt")), 0o755);
            assert_eq!(
                saved,
                read_working_tree_file(&repo, "a file.txt")
                    .await
                    .unwrap()
                    .version
            );
            // Saved again from there, as edits go on.
            save_working_tree_file(&repo, "a file.txt", "again\n", &expected(&saved, false))
                .await
                .unwrap();
            assert_eq!(
                fs::read_to_string(path.join("a file.txt")).unwrap(),
                "again\n"
            );
            let leftovers: Vec<_> = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .filter(|name| name.to_string_lossy().ends_with(".gitto"))
                .collect();
            assert_eq!(leftovers, Vec::<std::ffi::OsString>::new());
        });
    }

    #[test]
    fn keeps_crlf_line_ends_and_a_byte_order_mark() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("save-crlf").await;
            let path = repo.path();
            fs::write(path.join("crlf.txt"), "\u{FEFF}one\r\ntwo\r\n").unwrap();
            let read = read_working_tree_file(&repo, "crlf.txt").await.unwrap();

            // Edited as git shows it with `core.autocrlf`: with LF line ends.
            let edited = read.contents.replace("\r\n", "\n").replacen("two", "2", 1);
            save_working_tree_file(&repo, "crlf.txt", &edited, &expected(&read.version, false))
                .await
                .unwrap();
            assert_eq!(
                fs::read_to_string(path.join("crlf.txt")).unwrap(),
                "\u{FEFF}one\r\n2\r\n"
            );
        });
    }

    #[test]
    fn wont_save_over_a_file_that_changed_on_disk_since_unless_told_to() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("save-changed").await;
            let path = repo.path();
            let version = read_working_tree_file(&repo, "a file.txt")
                .await
                .unwrap()
                .version;
            fs::write(path.join("a file.txt"), "from an editor\n").unwrap();

            let error =
                save_working_tree_file(&repo, "a file.txt", "mine\n", &expected(&version, false))
                    .await
                    .unwrap_err();
            assert!(is_changed_on_disk(&error), "{error:?}");
            assert_eq!(
                fs::read_to_string(path.join("a file.txt")).unwrap(),
                "from an editor\n"
            );
            save_working_tree_file(&repo, "a file.txt", "mine\n", &expected(&version, true))
                .await
                .unwrap();
            assert_eq!(
                fs::read_to_string(path.join("a file.txt")).unwrap(),
                "mine\n"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn wont_save_through_a_link_outside_the_working_tree_or_over_a_file_that_isnt_utf_8() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("save-refused").await;
            let path = repo.path();
            symlink(path.join("c.txt"), path.join("link"));
            fs::write(path.join("latin1.txt"), [0x63, 0x61, 0x66, 0xe9]).unwrap();
            let save = |name: &str| save_working_tree_file(&repo, name, "x\n", &expected("", true));

            assert!(is_outside(&save("link").await.unwrap_err()));
            assert!(is_outside(&save("../outside.txt").await.unwrap_err()));
            assert!(is_outside(&save(".git/config").await.unwrap_err()));
            assert!(matches!(
                save("latin1.txt").await.unwrap_err(),
                Error::NotUtf8
            ));
            assert_eq!(fs::read_to_string(path.join("c.txt")).unwrap(), "b\n");
        });
    }

    #[test]
    fn takes_a_save_over_the_version_an_earlier_save_of_its_own_started_from() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("save-chained").await;
            let path = repo.path();
            let version = read_working_tree_file(&repo, "a file.txt")
                .await
                .unwrap()
                .version;
            // Both sent before either's done, as the last edits are when the window closes.
            let (first, second) = futures::join!(
                save_working_tree_file(&repo, "a file.txt", "first\n", &expected(&version, false)),
                save_working_tree_file(&repo, "a file.txt", "second\n", &expected(&version, false)),
            );
            first.unwrap();
            second.unwrap();
            assert_eq!(
                fs::read_to_string(path.join("a file.txt")).unwrap(),
                "second\n"
            );
            // Not once something else wrote it since.
            fs::write(path.join("a file.txt"), "from an editor\n").unwrap();
            let error =
                save_working_tree_file(&repo, "a file.txt", "third\n", &expected(&version, false))
                    .await
                    .unwrap_err();
            assert!(is_changed_on_disk(&error), "{error:?}");
        });
    }

    #[test]
    fn makes_a_file_that_was_deleted_again_when_overwriting_and_only_then() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("save-deleted").await;
            let path = repo.path();
            let version = read_working_tree_file(&repo, "a file.txt")
                .await
                .unwrap()
                .version;
            fs::remove_file(path.join("a file.txt")).unwrap();

            let error =
                save_working_tree_file(&repo, "a file.txt", "mine\n", &expected(&version, false))
                    .await
                    .unwrap_err();
            assert!(
                matches!(
                    error,
                    Error::FileChangedOnDisk {
                        how: DiskChange::Deleted,
                        ..
                    }
                ),
                "{error:?}"
            );
            assert!(error.to_string().contains("was deleted"), "{error}");
            save_working_tree_file(&repo, "a file.txt", "mine\n", &expected(&version, true))
                .await
                .unwrap();
            assert_eq!(
                fs::read_to_string(path.join("a file.txt")).unwrap(),
                "mine\n"
            );
            // A new file's permissions, not anyone's to write.
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                // What the umask leaves of 0o666, read without changing it (`umask` sets it, for
                // every thread): from a file made with it.
                let probe = fx.root().join("umask-probe");
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o666)
                    .open(&probe)
                    .unwrap();
                assert_eq!(mode(&path.join("a file.txt")), mode(&probe));
            }
        });
    }
}
