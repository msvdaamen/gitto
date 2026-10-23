//! Staging and unstaging files whole, everything at once, or some lines of a file. Ported from
//! `commands.ts`.

use std::sync::LazyLock;

use regex::Regex;

use super::lines::{LineAction, lines_patch};
use super::patch_version::patch_version;
use super::schema::LineSelection;
use crate::core::errors::{Error, Result};
use crate::core::repo::Repo;
use crate::core::runner::RunOptions;
use crate::features::conflicts::commands::refuse_conflict_markers;
use crate::features::diff::commands::{
    get_staged_file_patch, get_staged_file_patch_bytes, get_unstaged_file_patch,
    get_unstaged_file_patch_bytes,
};
use crate::input::{FileInput, Paths};

/// Paths go to git's stdin, NUL-separated, rather than on the command line, which holds only so
/// many (32k characters on Windows).
pub const PATHS_FROM_STDIN: [&str; 2] = ["--pathspec-from-file=-", "--pathspec-file-nul"];

/// Stages the files at `paths`, whole. A conflicted one is marked resolved by that, so it isn't
/// while it still has conflict markers: fails with `Error::ConflictMarkers`, and stages none.
pub async fn stage(repo: &Repo, paths: &Paths) -> Result<()> {
    let paths = paths.as_slice();
    repo.exclusive(|run| async move {
        refuse_conflict_markers(repo, &run, Some(paths)).await?;
        let mut args = vec!["add"];
        args.extend(PATHS_FROM_STDIN);
        run.run_with(args, RunOptions::default().stdin(nul_separated(paths)))
            .await?;
        Ok(())
    })
    .await
}

/// Unstages the files at `paths`, whole; their changes stay in the working tree.
pub async fn unstage(repo: &Repo, paths: &Paths) -> Result<()> {
    let paths = paths.as_slice();
    let args = unstage_args(repo.has_head().await);
    repo.write_with(args, RunOptions::default().stdin(nul_separated(paths)))
        .await?;
    Ok(())
}

/// The command that unstages the paths given on its stdin.
pub fn unstage_args(has_head: bool) -> Vec<&'static str> {
    // `restore --staged` resets to HEAD, so it can't run before the first commit.
    let mut args = if has_head {
        vec!["restore", "--staged"]
    } else {
        vec!["rm", "--cached", "-r", "-q"]
    };
    args.extend(PATHS_FROM_STDIN);
    args
}

/// `paths`, each ended by a NUL, for git's stdin (see `PATHS_FROM_STDIN`).
pub fn nul_separated<S: AsRef<str>>(paths: &[S]) -> String {
    paths
        .iter()
        .map(|path| format!("{}\0", path.as_ref()))
        .collect()
}

/// Stages every change, untracked files included; conflicts too, unless markers are left (see
/// `stage`).
pub async fn stage_all(repo: &Repo) -> Result<()> {
    repo.exclusive(|run| async move {
        refuse_conflict_markers(repo, &run, None).await?;
        run.run(["add", "-A"]).await?;
        Ok(())
    })
    .await
}

/// Unstages every staged change; conflicts stay conflicted.
pub async fn unstage_all(repo: &Repo) -> Result<()> {
    if !repo.has_head().await {
        // Nothing to unstage isn't an error.
        repo.write(["rm", "--cached", "-r", "-q", "--ignore-unmatch", "."])
            .await?;
        return Ok(());
    }
    // Resetting the whole index (`restore --staged .`, `reset`) would also undo conflicts, and
    // plain `reset` even abort the merge. So only the staged changes that aren't conflicts are
    // undone: `u` leaves out unmerged paths, and without renames both sides of one are listed.
    // Each is put back in the index as HEAD has it. Not with `restore --staged` and their paths:
    // git looks every file in the index up in the whole list, which took 6.9s for 40,000 staged
    // files in vscode, against 60ms this way. As one write, so stages still waiting to run are
    // seen, and none comes in between.
    repo.exclusive(|run| async move {
        // As bytes: a path goes back to git as it came, also one that isn't UTF-8.
        let staged = run
            .run_bytes([
                "diff",
                "--cached",
                "--raw",
                "-z",
                "--no-abbrev",
                "--no-renames",
                "--diff-filter=u",
            ])
            .await?;
        // `:<mode in HEAD> <mode> <object in HEAD> <object> <status>`, then the path.
        let fields: Vec<&[u8]> = staged.split(|&byte| byte == 0).collect();
        let mut entries = Vec::new();
        let mut i = 0;
        while i + 1 < fields.len() {
            let info = fields[i].get(1..).unwrap_or_default();
            let mut parts = info.split(|&byte| byte == b' ');
            let mode = parts.next().unwrap_or_default();
            let object = parts.nth(1).unwrap_or_default();
            // A file that isn't in HEAD has mode 0 there, which takes it out of the index.
            entries.extend_from_slice(mode);
            entries.push(b' ');
            entries.extend_from_slice(object);
            entries.push(b'\t');
            entries.extend_from_slice(fields[i + 1]);
            entries.push(0);
            i += 2;
        }
        if !entries.is_empty() {
            run.run_bytes_with(
                ["update-index", "-z", "--index-info"],
                RunOptions::default().stdin(entries),
            )
            .await?;
        }
        Ok(())
    })
    .await
}

/// Stages the lines `selection` picks from a file's unstaged changes, which the user picked them
/// from in the patch named `version` (see `patch_version`, and `get_unstaged_file_patch`). Fails
/// with `Error::PatchChanged` if that isn't the file's unstaged changes' patch any more: the lines
/// picked could be others by now. Resolves to the unstaged changes' patch the lines leave, to show
/// in its place. `untracked` is whether the file is, as its list has it.
pub async fn stage_lines(
    repo: &Repo,
    file: &FileInput,
    untracked: bool,
    version: &str,
    selection: &LineSelection,
) -> Result<String> {
    apply_lines(
        repo,
        version,
        selection,
        LineAction::Stage,
        // In the index once lines of it are staged, an untracked file is compared to that.
        || get_unstaged_file_patch_bytes(repo, file, untracked),
        || get_unstaged_file_patch(repo, file, false),
    )
    .await
}

/// Unstages the lines `selection` picks from a file's staged changes; see `stage_lines`. Resolves
/// to the staged changes' patch they leave.
pub async fn unstage_lines(
    repo: &Repo,
    file: &FileInput,
    version: &str,
    selection: &LineSelection,
) -> Result<String> {
    apply_lines(
        repo,
        version,
        selection,
        LineAction::Unstage,
        || get_staged_file_patch_bytes(repo, file),
        || get_staged_file_patch(repo, file),
    )
    .await
}

/// What `git apply --cached` says of a patch that isn't of the index as it is.
static INDEX_CHANGED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("patch does not apply|does not exist in index|already exists in index")
        .expect("a valid regex")
});

/// `bytes` read as Latin-1, one character per byte, as TS's `binary` read them: the patch is built
/// from them as text, and goes back to git as the same bytes (see `latin1_bytes`).
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

/// The bytes of `text`, which `latin1` made, with ASCII added: one per character.
fn latin1_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|char| u8::try_from(u32::from(char)).unwrap_or(b'?'))
        .collect()
}

/// Applies the patch that stages or unstages the lines picked from the patch named `version`,
/// built from the file's patch as git has it now, `read_bytes`: as one write, so no other one
/// changes the index in between. The patch is read as bytes, which go back to git as they came,
/// also for a file that isn't UTF-8: one whose lines don't survive being read as it, as the user's
/// were; it's named as the UI got it, as text. Resolves to the patch read once they're applied,
/// as text (`read_text`), which saves the UI asking for it.
async fn apply_lines<B, BF, T, TF>(
    repo: &Repo,
    version: &str,
    selection: &LineSelection,
    action: LineAction,
    read_bytes: B,
    read_text: T,
) -> Result<String>
where
    B: FnOnce() -> BF,
    BF: Future<Output = Result<Vec<u8>>>,
    T: FnOnce() -> TF,
    TF: Future<Output = Result<String>>,
{
    let verb = action.verb();
    repo.exclusive(|run| async move {
        let patch = read_bytes().await?;
        if patch_version(&String::from_utf8_lossy(&patch)) != version {
            return Err(Error::PatchChanged { action: verb });
        }
        let Some(lines) = lines_patch(&latin1(&patch), selection, action)? else {
            return Err(Error::LinesNotStageable(format!(
                "None of the selected lines are changes to be {verb}."
            )));
        };
        let mut args = vec![
            "apply",
            "--cached",
            // Neither warned about nor fixed, whatever `apply.whitespace` says: the lines are
            // staged as they are.
            "--whitespace=nowarn",
        ];
        if action == LineAction::Unstage {
            args.push("--reverse");
        }
        args.push("-");
        let applied = run
            .run_bytes_with(
                args,
                RunOptions::default()
                    .stdin(latin1_bytes(&lines))
                    // Unchanged lines have to match the index's exactly, whatever
                    // `apply.ignoreWhitespace` says: they're checked to be the ones the lines were
                    // picked among.
                    .config("apply.ignoreWhitespace=no"),
            )
            .await;
        if let Err(error) = applied {
            // The index changed since the patch was read, e.g. by a git command run in a
            // terminal.
            if let Some(git) = error.as_git()
                && INDEX_CHANGED.is_match(&git.stderr)
            {
                return Err(Error::PatchChanged { action: verb });
            }
            return Err(error);
        }
        read_text().await
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    use super::super::schema::LineRange;
    use super::super::test_status::{StatusFile, status_files};
    use super::*;
    use crate::core::errors::GitErrorKind;
    use crate::core::repo::Observed;
    use crate::features::diff::schema::ChangedFile;
    use crate::features::status::commands::get_status;
    use crate::features::status::schema::WorkingTreeFiles;
    use crate::schema::FileStatus::{self, *};
    use crate::test_support::{Fixture, git, non_utf8_names};

    fn strings(paths: &[&str]) -> Paths {
        Paths::parse(paths.iter().copied()).unwrap()
    }

    fn file(path: &str, orig_path: Option<&str>) -> FileInput {
        FileInput {
            path: path.to_owned(),
            orig_path: orig_path.map(str::to_owned),
        }
    }

    /// `stage_lines`, given the patch the lines were picked from, `shown`, rather than its version.
    async fn stage_lines_of(
        repo: &Repo,
        path: &str,
        orig_path: Option<&str>,
        untracked: bool,
        shown: &str,
        selection: LineSelection,
    ) -> Result<String> {
        stage_lines(
            repo,
            &file(path, orig_path),
            untracked,
            &patch_version(shown),
            &selection,
        )
        .await
    }

    /// `unstage_lines`, given the patch the lines were picked from, `shown`, rather than its
    /// version.
    async fn unstage_lines_of(
        repo: &Repo,
        path: &str,
        orig_path: Option<&str>,
        shown: &str,
        selection: LineSelection,
    ) -> Result<String> {
        unstage_lines(
            repo,
            &file(path, orig_path),
            &patch_version(shown),
            &selection,
        )
        .await
    }

    /// A file's unstaged patch, as the UI has it.
    async fn unstaged_patch(
        repo: &Repo,
        path: &str,
        untracked: bool,
        orig_path: Option<&str>,
    ) -> String {
        get_unstaged_file_patch(repo, &file(path, orig_path), untracked)
            .await
            .unwrap()
    }

    /// A file's staged patch, as the UI has it.
    async fn staged_patch(repo: &Repo, path: &str, orig_path: Option<&str>) -> String {
        get_staged_file_patch(repo, &file(path, orig_path))
            .await
            .unwrap()
    }

    fn entry(path: &str, staged: Option<FileStatus>, unstaged: Option<FileStatus>) -> StatusFile {
        StatusFile::new(path, None, staged, unstaged)
    }

    fn changed_file(
        path: &str,
        status: FileStatus,
        additions: Option<u64>,
        deletions: Option<u64>,
    ) -> ChangedFile {
        ChangedFile {
            path: path.to_owned(),
            status,
            orig_path: None,
            additions,
            deletions,
        }
    }

    fn is_index_locked(error: &Error) -> bool {
        error
            .as_git()
            .is_some_and(|git| git.kind == GitErrorKind::IndexLocked)
    }

    // Staging files.

    #[test]
    fn moves_staged_changes_between_the_working_tree_and_the_index() {
        let fx = Fixture::new();
        smol::block_on(async {
            let repo = fx.create_history_repo("history").await;
            let change = changed_file("a file.txt", Modified, Some(1), Some(2));
            let untracked_file = changed_file("new file.txt", Untracked, None, None);

            stage(&repo, &strings(&["a file.txt"])).await.unwrap();
            assert_eq!(
                get_status(&repo).await.unwrap().changes,
                WorkingTreeFiles {
                    staged: vec![change.clone()],
                    unstaged: vec![untracked_file.clone()],
                    uncounted: false,
                    marker_free: vec![],
                }
            );

            unstage(&repo, &strings(&["a file.txt"])).await.unwrap();
            assert_eq!(
                get_status(&repo).await.unwrap().changes,
                WorkingTreeFiles {
                    staged: vec![],
                    unstaged: vec![change, untracked_file],
                    uncounted: false,
                    marker_free: vec![],
                }
            );
        });
    }

    #[test]
    fn stages_and_unstages_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("empty");
        smol::block_on(async {
            let repo = fx.repos.open("empty").await.unwrap();
            fs::write(path.join("x y.txt"), "hi\n").unwrap();

            stage(&repo, &strings(&["x y.txt"])).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                [entry("x y.txt", Some(Added), None)]
            );
            assert_eq!(
                get_status(&repo).await.unwrap().changes,
                WorkingTreeFiles {
                    staged: vec![changed_file("x y.txt", Added, Some(1), Some(0))],
                    unstaged: vec![],
                    uncounted: false,
                    marker_free: vec![],
                }
            );

            unstage(&repo, &strings(&["x y.txt"])).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                [entry("x y.txt", None, Some(Untracked))]
            );
        });
    }

    #[test]
    fn recognises_another_git_process_holding_the_index() {
        let fx = Fixture::new();
        let path = fx.create_repo("locked");
        smol::block_on(async {
            let repo = fx.repos.open("locked").await.unwrap();
            fs::write(path.join("file.txt"), "x").unwrap();
            fs::write(path.join(".git").join("index.lock"), "").unwrap();
            let error = stage(&repo, &strings(&["file.txt"])).await.unwrap_err();
            assert!(is_index_locked(&error), "{error:?}");
        });
    }

    #[test]
    fn refuses_no_paths_or_an_empty_one() {
        // Refused as the input's parsed: `stage` and `unstage` take `Paths`.
        for paths in [vec![], vec![String::new()]] {
            let error = Paths::parse(paths).unwrap_err();
            assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
        }
    }

    // A big working tree.

    #[test]
    fn stages_and_unstages_more_paths_than_fit_on_a_windows_command_line() {
        let fx = Fixture::new();
        let path = fx.create_repo("big-stage");
        let names: Vec<String> = (0..1000)
            .map(|i| format!("{}{i}.txt", "long-file-name-".repeat(3)))
            .collect();
        for name in &names {
            fs::write(path.join(name), "a\n").unwrap();
        }
        smol::block_on(async {
            let repo = fx.repos.open("big-stage").await.unwrap();

            let names = Paths::parse(names).unwrap();
            stage(&repo, &names).await.unwrap();
            assert_eq!(get_status(&repo).await.unwrap().changes.staged.len(), 1000);
            unstage(&repo, &names).await.unwrap();
            assert_eq!(get_status(&repo).await.unwrap().changes.staged, []);
        });
    }

    #[test]
    fn stages_and_unstages_everything_at_once() {
        let fx = Fixture::new();
        let path = fx.create_repo("all");
        for name in ["kept.txt", "deleted.txt", "moved.txt"] {
            fs::write(path.join(name), format!("{name}\n")).unwrap();
        }
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "First"]);
        fs::write(path.join("kept.txt"), "changed\n").unwrap();
        fs::remove_file(path.join("deleted.txt")).unwrap();
        git(&path, ["mv", "moved.txt", "renamed.txt"]);
        fs::write(path.join("new file.txt"), "new\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("all").await.unwrap();

            stage_all(&repo).await.unwrap();
            let files: Vec<_> = status_files(&repo)
                .await
                .into_iter()
                .map(|file| (file.path, file.staged, file.unstaged))
                .collect();
            assert_eq!(
                files,
                [
                    ("deleted.txt".to_owned(), Some(Deleted), None),
                    ("kept.txt".to_owned(), Some(Modified), None),
                    ("new file.txt".to_owned(), Some(Added), None),
                    ("renamed.txt".to_owned(), Some(Renamed), None),
                ]
            );

            unstage_all(&repo).await.unwrap();
            assert_eq!(get_status(&repo).await.unwrap().changes.staged, []);
            let files: Vec<_> = status_files(&repo)
                .await
                .into_iter()
                .map(|file| (file.path, file.unstaged))
                .collect();
            assert_eq!(
                files,
                [
                    ("deleted.txt".to_owned(), Some(Deleted)),
                    ("kept.txt".to_owned(), Some(Modified)),
                    ("moved.txt".to_owned(), Some(Deleted)),
                    ("new file.txt".to_owned(), Some(Untracked)),
                    ("renamed.txt".to_owned(), Some(Untracked)),
                ]
            );
        });
    }

    #[test]
    fn unstages_everything_but_conflicts_which_stay_conflicted() {
        let fx = Fixture::new();
        let path = fx.create_repo("all-conflict");
        fs::write(path.join("both.txt"), "base\n").unwrap();
        fs::write(path.join("other.txt"), "other\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "base"]);
        git(&path, ["checkout", "-q", "-b", "side"]);
        fs::write(path.join("both.txt"), "side\n").unwrap();
        git(&path, ["commit", "-q", "-am", "side"]);
        git(&path, ["checkout", "-q", "main"]);
        fs::write(path.join("both.txt"), "main\n").unwrap();
        git(&path, ["commit", "-q", "-am", "main"]);
        assert_ne!(
            crate::test_support::conflicts::try_git(&path, ["merge", "-q", "side"]),
            0
        );
        fs::write(path.join("other.txt"), "changed\n").unwrap();
        git(&path, ["add", "other.txt"]);
        smol::block_on(async {
            let repo = fx.repos.open("all-conflict").await.unwrap();

            unstage_all(&repo).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                [
                    entry("other.txt", None, Some(Modified)),
                    StatusFile::conflicted("both.txt", "UU", "100644", "100644"),
                ]
            );
            assert!(path.join(".git").join("MERGE_HEAD").exists());
        });
    }

    #[test]
    #[cfg(unix)]
    fn unstages_a_folder_that_replaced_a_file_a_symlink_and_a_file_made_executable() {
        let fx = Fixture::new();
        let path = fx.create_repo("all-kinds");
        fs::write(path.join("thing"), "a file\n").unwrap();
        fs::write(path.join("script.sh"), "echo\n").unwrap();
        fs::write(path.join("tab\tand\nline.txt"), "odd name\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "First"]);
        fs::remove_file(path.join("thing")).unwrap();
        fs::create_dir(path.join("thing")).unwrap();
        fs::write(path.join("thing").join("inner.txt"), "now a folder\n").unwrap();
        std::os::unix::fs::symlink("script.sh", path.join("link")).unwrap();
        git(&path, ["update-index", "--chmod=+x", "script.sh"]);
        fs::write(path.join("tab\tand\nline.txt"), "changed\n").unwrap();
        git(&path, ["add", "."]);
        let head = git(&path, ["ls-tree", "-r", "HEAD"]);
        smol::block_on(async {
            let repo = fx.repos.open("all-kinds").await.unwrap();

            unstage_all(&repo).await.unwrap();
            assert_eq!(get_status(&repo).await.unwrap().changes.staged, []);
            // The index is HEAD's again: modes, objects and all.
            assert_eq!(
                git(&path, ["ls-files", "--stage"]).replace(" 0\t", "\t"),
                head.replace(" blob ", " ")
            );
            assert_eq!(
                fs::read_to_string(path.join("thing").join("inner.txt")).unwrap(),
                "now a folder\n"
            );
        });
    }

    #[test]
    fn unstages_a_file_whose_name_isnt_utf_8() {
        if !non_utf8_names() {
            return;
        }
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;

            let fx = Fixture::new();
            let path = fx.create_repo("all-latin1");
            // "café.txt" in Latin-1: read as UTF-8, its é doesn't come back as the byte it was.
            let file = path.join(OsStr::from_bytes(b"caf\xe9.txt"));
            fs::write(&file, "a\n").unwrap();
            git(&path, ["add", "."]);
            git(&path, ["commit", "-q", "-m", "First"]);
            fs::write(&file, "changed\n").unwrap();
            git(&path, ["add", "."]);
            smol::block_on(async {
                let repo = fx.repos.open("all-latin1").await.unwrap();

                unstage_all(&repo).await.unwrap();
                // Nothing staged, and no entry under another name.
                assert_eq!(git(&path, ["diff", "--cached", "--name-only"]), "");
                assert_eq!(
                    git(&path, ["ls-files", "-z"])
                        .split('\0')
                        .filter(|name| !name.is_empty())
                        .count(),
                    1
                );
                // The change is still there, unstaged.
                assert_ne!(git(&path, ["diff", "--name-only"]), "");
            });
        }
    }

    #[test]
    fn unstages_everything_without_giving_git_the_staged_files_as_paths_to_look_up() {
        let fx = Fixture::new();
        let path = fx.create_repo("all-unlisted");
        fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-q", "-m", "First"]);
        fs::write(path.join("a.txt"), "changed\n").unwrap();
        fs::write(path.join("b.txt"), "b\n").unwrap();
        git(&path, ["add", "."]);
        smol::block_on(async {
            let repo = fx.repos.open("all-unlisted").await.unwrap();

            // Git looks every file in the index up in such a list: seconds, for thousands of them.
            let commands = Arc::new(Mutex::new(Vec::<String>::new()));
            let seen = Arc::clone(&commands);
            let watched = repo.observed(move |observed| {
                if let Observed::Ran { args, .. } = observed {
                    seen.lock().unwrap().push(args.join(" "));
                }
            });
            unstage_all(&watched).await.unwrap();
            let pathspecs: Vec<String> = commands
                .lock()
                .unwrap()
                .iter()
                .filter(|command| command.contains("pathspec"))
                .cloned()
                .collect();
            assert_eq!(pathspecs, Vec::<String>::new());
            assert!(!commands.lock().unwrap().is_empty());
            assert_eq!(
                status_files(&repo).await,
                [
                    entry("a.txt", None, Some(Modified)),
                    entry("b.txt", None, Some(Untracked)),
                ]
            );
        });
    }

    #[test]
    fn stages_and_unstages_everything_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("all-empty");
        smol::block_on(async {
            let repo = fx.repos.open("all-empty").await.unwrap();
            unstage_all(&repo).await.unwrap();

            fs::write(path.join("a.txt"), "a\n").unwrap();
            stage_all(&repo).await.unwrap();
            assert_eq!(get_status(&repo).await.unwrap().changes.staged.len(), 1);
            unstage_all(&repo).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                [entry("a.txt", None, Some(Untracked))]
            );
        });
    }

    #[test]
    fn reports_a_locked_index_when_staging_more_paths_than_git_reads_before_it_fails() {
        let fx = Fixture::new();
        let path = fx.create_repo("big-locked");
        smol::block_on(async {
            let repo = fx.repos.open("big-locked").await.unwrap();
            fs::write(path.join(".git").join("index.lock"), "").unwrap();
            // About 1MB: more than the pipe holds, so git exits before taking it all from stdin.
            let names: Vec<String> = (0..20_000)
                .map(|i| format!("{}{i}", "long-file-name-".repeat(3)))
                .collect();
            let error = stage(&repo, &Paths::parse(names).unwrap())
                .await
                .unwrap_err();
            assert!(is_index_locked(&error), "{error:?}");
        });
    }

    // Staging lines.

    /// Every changed line of `patch`: removed ones by their old number, added ones by their new
    /// one.
    fn all_lines(patch: &str) -> LineSelection {
        static HEADER: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^@@ -([0-9]+)(?:,[0-9]+)? \+([0-9]+)").unwrap());
        let mut selection = LineSelection::default();
        let mut old_line = 0;
        let mut new_line = 0;
        for line in patch.split('\n') {
            if let Some(header) = HEADER.captures(line) {
                old_line = header[1].parse().unwrap();
                new_line = header[2].parse().unwrap();
            } else if line.starts_with(' ') {
                old_line += 1;
                new_line += 1;
            } else if line.starts_with('-') && !line.starts_with("--- ") {
                selection
                    .deletions
                    .push(LineRange::new(old_line, old_line).unwrap());
                old_line += 1;
            } else if line.starts_with('+') && !line.starts_with("+++ ") {
                selection
                    .additions
                    .push(LineRange::new(new_line, new_line).unwrap());
                new_line += 1;
            }
        }
        selection
    }

    /// Single lines: removed ones by their old number, added ones by their new one.
    fn lines(deletions: &[u64], additions: &[u64]) -> LineSelection {
        let single = |line: &u64| LineRange::new(*line, *line).unwrap();
        LineSelection {
            deletions: deletions.iter().map(single).collect(),
            additions: additions.iter().map(single).collect(),
        }
    }

    /// A file as the index has it.
    fn indexed(path: &Path, file: &str) -> String {
        git(path, ["show", &format!(":{file}")])
    }

    /// A file as the index has it, as bytes, untrimmed.
    fn indexed_bytes(path: &Path, file: &str) -> Vec<u8> {
        let output = Command::new("git")
            .args(["show", &format!(":{file}")])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    }

    /// `text`'s bytes in Latin-1, as a file that isn't UTF-8 has them.
    fn latin1_of(text: &str) -> Vec<u8> {
        text.chars().map(|char| char as u8).collect()
    }

    fn is_patch_changed(error: &Error) -> bool {
        matches!(error, Error::PatchChanged { .. })
    }

    #[test]
    fn stages_lines_of_one_hunk_leaving_the_rest_unstaged() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-hunks");
        let before: Vec<String> = (1..=20).map(|i| i.to_string()).collect();
        fs::write(path.join("f.txt"), format!("{}\n", before.join("\n"))).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        let after: Vec<&str> = before
            .iter()
            .map(|line| match line.as_str() {
                "3" => "three",
                "17" => "seventeen",
                line => line,
            })
            .collect();
        fs::write(path.join("f.txt"), format!("{}\n", after.join("\n"))).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-hunks").await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            // The second hunk: 17 replaced.
            let left = stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[17], &[17]))
                .await
                .unwrap();
            assert_eq!(
                indexed(&path, "f.txt").split('\n').nth(16),
                Some("seventeen")
            );
            assert_eq!(indexed(&path, "f.txt").split('\n').nth(2), Some("3"));
            // The unstaged changes left, as a fresh read has them.
            assert_eq!(left, unstaged_patch(&repo, "f.txt", false, None).await);
            assert!(left.contains("-3\n+three\n"), "{left}");
            assert!(!left.contains("seventeen"), "{left}");

            // And back.
            let staged = staged_patch(&repo, "f.txt", None).await;
            assert_eq!(
                unstage_lines_of(&repo, "f.txt", None, &staged, all_lines(&staged))
                    .await
                    .unwrap(),
                ""
            );
            assert_eq!(git(&path, ["diff", "--cached", "--name-only"]), "");
        });
    }

    #[test]
    fn stages_nothing_from_a_patch_that_isnt_the_files_any_more() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-stale");
        fs::write(path.join("f.txt"), "a\nb\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("f.txt"), "a\nB\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-stale").await.unwrap();
            let shown = unstaged_patch(&repo, "f.txt", false, None).await;

            // Saved again since: the lines picked were of other changes.
            fs::write(path.join("f.txt"), "A\nB\n").unwrap();
            let error = stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[2], &[2]))
                .await
                .unwrap_err();
            assert!(is_patch_changed(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                "The file changed since its changes were shown, so nothing was staged."
            );
            // Staged since, in full: no changes left.
            git(&path, ["add", "."]);
            let error = stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[2], &[2]))
                .await
                .unwrap_err();
            assert!(is_patch_changed(&error), "{error:?}");
            assert_eq!(indexed(&path, "f.txt"), "A\nB");

            // Unstaging, the same.
            let staged = staged_patch(&repo, "f.txt", None).await;
            git(&path, ["reset", "-q"]);
            let error = unstage_lines_of(&repo, "f.txt", None, &staged, all_lines(&staged))
                .await
                .unwrap_err();
            assert!(is_patch_changed(&error), "{error:?}");
        });
    }

    #[test]
    fn says_when_none_of_the_lines_picked_are_changes() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-none");
        fs::write(path.join("f.txt"), "a\nb\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("f.txt"), "a\nB\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-none").await.unwrap();
            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            let error = stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[1], &[1]))
                .await
                .unwrap_err();
            assert!(matches!(error, Error::LinesNotStageable(_)), "{error:?}");
        });
    }

    #[test]
    fn stages_some_lines_of_an_untracked_file_as_a_new_file() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-untracked");
        fs::write(path.join("x y.txt"), "a\nb\nc\n").unwrap();
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        smol::block_on(async {
            let repo = fx.repos.open("lines-untracked").await.unwrap();

            let shown = unstaged_patch(&repo, "x y.txt", true, None).await;
            let left = stage_lines_of(&repo, "x y.txt", None, true, &shown, lines(&[], &[1, 3]))
                .await
                .unwrap();
            assert_eq!(indexed(&path, "x y.txt"), "a\nc");
            // Compared to what's staged of it now.
            assert_eq!(left, unstaged_patch(&repo, "x y.txt", false, None).await);
            assert!(left.contains("@@ -1,2 +1,3 @@\n a\n+b\n c\n"), "{left}");
            assert_eq!(
                status_files(&repo).await,
                [entry("x y.txt", Some(Added), Some(Modified))]
            );
        });
    }

    #[test]
    fn stages_some_lines_of_a_file_added_with_intent_to_add() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-intent");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        fs::write(path.join("f.txt"), "a\nb\n").unwrap();
        git(&path, ["add", "-N", "f.txt"]);
        smol::block_on(async {
            let repo = fx.repos.open("lines-intent").await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[], &[2]))
                .await
                .unwrap();
            assert_eq!(indexed(&path, "f.txt"), "b");
        });
    }

    #[test]
    fn unstages_some_lines_of_a_new_file_and_all_of_them_back_to_untracked() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-added");
        fs::write(path.join("f.txt"), "a\nb\nc\n").unwrap();
        git(&path, ["add", "."]);
        smol::block_on(async {
            let repo = fx.repos.open("lines-added").await.unwrap();

            // Before the first commit: the staged changes are compared to nothing.
            let shown = staged_patch(&repo, "f.txt", None).await;
            unstage_lines_of(&repo, "f.txt", None, &shown, lines(&[], &[2]))
                .await
                .unwrap();
            assert_eq!(indexed(&path, "f.txt"), "a\nc");
            let staged = staged_patch(&repo, "f.txt", None).await;
            unstage_lines_of(&repo, "f.txt", None, &staged, all_lines(&staged))
                .await
                .unwrap();
            assert_eq!(
                status_files(&repo).await,
                [entry("f.txt", None, Some(Untracked))]
            );
        });
    }

    #[test]
    fn stages_some_lines_of_a_deleted_file_keeping_the_rest() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-deleted");
        fs::write(path.join("f.txt"), "a\nb\nc\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::remove_file(path.join("f.txt")).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-deleted").await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[2], &[]))
                .await
                .unwrap();
            assert_eq!(indexed(&path, "f.txt"), "a\nc");
            let rest = unstaged_patch(&repo, "f.txt", false, None).await;
            stage_lines_of(&repo, "f.txt", None, false, &rest, all_lines(&rest))
                .await
                .unwrap();
            assert_eq!(
                status_files(&repo).await,
                [entry("f.txt", Some(Deleted), None)]
            );

            // Unstaging a line of the deletion puts it back in the index, alone.
            let staged = staged_patch(&repo, "f.txt", None).await;
            unstage_lines_of(&repo, "f.txt", None, &staged, lines(&[3], &[]))
                .await
                .unwrap();
            assert_eq!(indexed(&path, "f.txt"), "c");
        });
    }

    #[test]
    fn unstages_lines_of_a_staged_rename_which_stays_staged() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-renamed");
        let text = (0..10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path.join("old name.txt"), format!("{text}\n")).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        git(&path, ["mv", "old name.txt", "new name.txt"]);
        fs::write(
            path.join("new name.txt"),
            format!("{}\n", text.replacen("line 5", "line five", 1)),
        )
        .unwrap();
        git(&path, ["add", "."]);
        smol::block_on(async {
            let repo = fx.repos.open("lines-renamed").await.unwrap();

            let staged = staged_patch(&repo, "new name.txt", Some("old name.txt")).await;
            unstage_lines_of(
                &repo,
                "new name.txt",
                Some("old name.txt"),
                &staged,
                all_lines(&staged),
            )
            .await
            .unwrap();
            assert_eq!(
                status_files(&repo).await,
                [StatusFile::new(
                    "new name.txt",
                    Some("old name.txt"),
                    Some(Renamed),
                    Some(Modified)
                )]
            );
            assert_eq!(indexed(&path, "new name.txt"), text);
        });
    }

    #[test]
    fn stages_lines_of_a_rename_added_with_intent_to_add_with_the_rename() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-intent-rename");
        let text = (0..20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(path.join("old.txt"), format!("{text}\n")).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::remove_file(path.join("old.txt")).unwrap();
        let changed =
            text.replacen("line 2", "line two", 1)
                .replacen("line 17", "line seventeen", 1);
        fs::write(path.join("new.txt"), format!("{changed}\n")).unwrap();
        git(&path, ["add", "-N", "new.txt"]);
        smol::block_on(async {
            let repo = fx.repos.open("lines-intent-rename").await.unwrap();

            let shown = unstaged_patch(&repo, "new.txt", false, Some("old.txt")).await;
            assert!(shown.contains("rename from old.txt"), "{shown}");
            stage_lines_of(
                &repo,
                "new.txt",
                Some("old.txt"),
                false,
                &shown,
                lines(&[3], &[3]),
            )
            .await
            .unwrap();
            assert_eq!(
                status_files(&repo).await,
                [StatusFile::new(
                    "new.txt",
                    Some("old.txt"),
                    Some(Renamed),
                    Some(Modified)
                )]
            );
            assert_eq!(
                indexed(&path, "new.txt"),
                text.replacen("line 2", "line two", 1)
            );
        });
    }

    #[test]
    fn leaves_a_change_of_mode_where_it_is() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-mode");
        fs::write(path.join("run.sh"), "echo a\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("run.sh"), "echo b\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        smol::block_on(async {
            let repo = fx.repos.open("lines-mode").await.unwrap();

            let shown = unstaged_patch(&repo, "run.sh", false, None).await;
            stage_lines_of(&repo, "run.sh", None, false, &shown, all_lines(&shown))
                .await
                .unwrap();
            assert!(git(&path, ["ls-files", "-s", "run.sh"]).starts_with("100644 "));
            assert_eq!(indexed(&path, "run.sh"), "echo b");
            assert_eq!(
                unstaged_patch(&repo, "run.sh", false, None).await,
                "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n"
            );
        });
    }

    #[test]
    fn cant_stage_a_binary_files_changes_by_line() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-binary");
        fs::write(path.join("b.dat"), [0u8, 1, 2]).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("b.dat"), [0u8, 1, 3]).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-binary").await.unwrap();

            let shown = unstaged_patch(&repo, "b.dat", false, None).await;
            let error = stage_lines_of(&repo, "b.dat", None, false, &shown, lines(&[1], &[1]))
                .await
                .unwrap_err();
            assert!(matches!(error, Error::LinesNotStageable(_)), "{error:?}");
            assert_eq!(
                error.to_string(),
                "A binary file's changes can only be staged whole."
            );
        });
    }

    // Not in TS, where it passed as git printed it, read as Latin-1: an untracked file's patch
    // comes from a command that exits with 1, and isn't UTF-8 here.
    #[test]
    fn stages_lines_of_an_untracked_file_that_isnt_utf_8_as_they_are() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-untracked-latin1");
        git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
        fs::write(path.join("f.txt"), latin1_of("caf\u{e9}\nb\ncr\u{e8}me\n")).unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-untracked-latin1").await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", true, None).await;
            stage_lines_of(&repo, "f.txt", None, true, &shown, lines(&[], &[1, 3]))
                .await
                .unwrap();
            assert_eq!(
                indexed_bytes(&path, "f.txt"),
                latin1_of("caf\u{e9}\ncr\u{e8}me\n")
            );
        });
    }

    #[test]
    fn stages_lines_of_a_file_that_isnt_utf_8_as_they_are() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-latin1");
        // "café" in Latin-1: read as UTF-8, its é doesn't come back as the byte it was.
        fs::write(path.join("f.txt"), latin1_of("caf\u{e9}\nb\n")).unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(
            path.join("f.txt"),
            latin1_of("caf\u{e9}\nb\ncr\u{e8}me\nd\n"),
        )
        .unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-latin1").await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[], &[3]))
                .await
                .unwrap();
            assert_eq!(
                indexed_bytes(&path, "f.txt"),
                latin1_of("caf\u{e9}\nb\ncr\u{e8}me\n")
            );
        });
    }

    /// Stages lines of a file with CRLF line ends as git has them in the index, with
    /// `core.autocrlf` set to `autocrlf`: `end` is the line end the index has.
    fn stages_crlf_lines(autocrlf: &str, end: &str) {
        let fx = Fixture::new();
        let name = format!("lines-crlf-{autocrlf}");
        let path = fx.create_repo(&name);
        git(&path, ["config", "core.autocrlf", autocrlf]);
        fs::write(path.join("f.txt"), "a\r\nb\r\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("f.txt"), "a\r\nb\r\nc\r\nd\r\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open(&name).await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[], &[4]))
                .await
                .unwrap();
            assert_eq!(
                String::from_utf8(indexed_bytes(&path, "f.txt")).unwrap(),
                format!("a{end}b{end}d{end}")
            );
        });
    }

    #[test]
    fn stages_lines_of_a_file_with_crlf_line_ends_as_git_has_them_in_the_index_with_core_autocrlf_false()
     {
        stages_crlf_lines("false", "\r\n");
    }

    #[test]
    fn stages_lines_of_a_file_with_crlf_line_ends_as_git_has_them_in_the_index_with_core_autocrlf_true()
     {
        stages_crlf_lines("true", "\n");
    }

    #[test]
    fn stages_lines_as_they_are_whatever_the_users_settings_for_whitespace_say() {
        let fx = Fixture::new();
        let path = fx.create_repo("lines-whitespace");
        git(&path, ["config", "apply.whitespace", "error"]);
        git(&path, ["config", "apply.ignoreWhitespace", "change"]);
        fs::write(path.join("f.txt"), "a\nb\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        fs::write(path.join("f.txt"), "a  \nb\ntrailing  \n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("lines-whitespace").await.unwrap();

            let shown = unstaged_patch(&repo, "f.txt", false, None).await;
            stage_lines_of(&repo, "f.txt", None, false, &shown, lines(&[], &[3]))
                .await
                .unwrap();
            assert_eq!(
                String::from_utf8(indexed_bytes(&path, "f.txt")).unwrap(),
                "a\nb\ntrailing  \n"
            );
        });
    }

    #[test]
    fn refuses_a_line_numbered_0() {
        // Refused as the input's parsed: a `LineSelection` is of `LineRange`s.
        let error = LineRange::new(0, 0).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
    }

    // Staging lines, in any combination.

    /// A seeded pseudo-random number generator (mulberry32), so a failure can be run again.
    fn random(mut seed: u32) -> impl FnMut() -> f64 {
        move || {
            seed = seed.wrapping_add(0x6d2b_79f5);
            let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
            t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
            f64::from(t ^ (t >> 14)) / 4_294_967_296.0
        }
    }

    /// Some of `selection`'s lines, each with a chance of one in two; `None` if that's none, which
    /// there's nothing to stage of.
    fn some(selection: &LineSelection, next: &mut impl FnMut() -> f64) -> Option<LineSelection> {
        let deletions: Vec<LineRange> = selection
            .deletions
            .iter()
            .copied()
            .filter(|_| next() < 0.5)
            .collect();
        let additions: Vec<LineRange> = selection
            .additions
            .iter()
            .copied()
            .filter(|_| next() < 0.5)
            .collect();
        (deletions.len() + additions.len() > 0).then_some(LineSelection {
            deletions,
            additions,
        })
    }

    /// A file of `base`'s lines, some edited, removed or added, with or without a newline at its
    /// end, and with CRLF line ends or not.
    fn some_file(next: &mut impl FnMut() -> f64, base: &[String], crlf: bool) -> String {
        let mut kept: Vec<String> = Vec::new();
        for line in base {
            let roll = next();
            if roll < 0.1 {
                continue;
            }
            if roll < 0.2 {
                kept.push(format!("{line} edited"));
            } else {
                kept.push(line.clone());
            }
            if next() < 0.1 {
                kept.push(format!("added after {line}"));
            }
        }
        if next() < 0.2 {
            kept.insert(0, "added first".to_owned());
        }
        let end = if crlf { "\r\n" } else { "\n" };
        let text = kept.join(end);
        if next() < 0.3 || text.is_empty() {
            text
        } else {
            format!("{text}{end}")
        }
    }

    #[test]
    fn stages_some_lines_then_the_rest_to_the_working_trees_file_and_unstages_them_back() {
        // A dozen files, about 12 git commands each: a few seconds. Passed for 400 when it was
        // written.
        let fx = Fixture::new();
        let path = fx.create_repo("lines-random");
        smol::block_on(async {
            let repo = fx.repos.open("lines-random").await.unwrap();
            git(&path, ["commit", "-q", "--allow-empty", "-m", "first"]);
            for seed in 1..=12 {
                let mut next = random(seed);
                let crlf = next() < 0.2;
                let length = 5 + (next() * 40.0).floor() as usize;
                let base: Vec<String> = (0..length).map(|i| format!("line {i}")).collect();
                let before = some_file(&mut next, &base, crlf);
                let after = some_file(&mut next, &base, crlf);
                if before == after {
                    continue;
                }
                fs::write(path.join("f.txt"), &before).unwrap();
                git(&path, ["add", "."]);
                git(
                    &path,
                    [
                        "commit",
                        "-q",
                        "--allow-empty",
                        "-m",
                        &format!("seed {seed}"),
                    ],
                );
                fs::write(path.join("f.txt"), &after).unwrap();
                let context = format!("seed {seed}");

                let shown = unstaged_patch(&repo, "f.txt", false, None).await;
                if let Some(picked) = some(&all_lines(&shown), &mut next) {
                    stage_lines_of(&repo, "f.txt", None, false, &shown, picked)
                        .await
                        .expect(&context);
                }
                let rest = unstaged_patch(&repo, "f.txt", false, None).await;
                if !rest.is_empty() {
                    stage_lines_of(&repo, "f.txt", None, false, &rest, all_lines(&rest))
                        .await
                        .expect(&context);
                }
                assert_eq!(git(&path, ["diff", "--name-only"]), "", "{context}");

                let added = staged_patch(&repo, "f.txt", None).await;
                if let Some(unpicked) = some(&all_lines(&added), &mut next) {
                    unstage_lines_of(&repo, "f.txt", None, &added, unpicked)
                        .await
                        .expect(&context);
                }
                let left = staged_patch(&repo, "f.txt", None).await;
                if !left.is_empty() {
                    unstage_lines_of(&repo, "f.txt", None, &left, all_lines(&left))
                        .await
                        .expect(&context);
                }
                assert_eq!(
                    git(&path, ["diff", "--cached", "--name-only"]),
                    "",
                    "{context}"
                );
            }
        });
    }
}
