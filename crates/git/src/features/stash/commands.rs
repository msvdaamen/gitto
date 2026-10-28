//! Listing, pushing, popping and dropping stashes, and reading what they changed. Ported from
//! `commands.ts`.

use std::sync::LazyLock;

use regex::Regex;

use super::parse::{STASH_LIST_FORMAT, parse_stash_list};
use super::schema::Stash;
use crate::core::errors::{Error, Result};
use crate::core::repo::{GitCommand, Repo, has_conflicts, resolve_ref, succeeds};
use crate::core::runner::RunOptions;
use crate::features::diff::commands::{PORCELAIN_DIFF, get_commit_file_patch, read_patch};
use crate::features::diff::parse::parse_diff;
use crate::features::diff::schema::ChangedFile;
use crate::input::{FileInput, Sha};

/// For the stash commands, which take no paths: with literal pathspecs, `stash push
/// --include-untracked` leaves the untracked files it stashed behind, as it removes them with a
/// glob.
fn stash_env() -> RunOptions {
    RunOptions::default().glob_pathspecs()
}

/// For a stash or a pop, which put the changed files back as they are in HEAD or in the stash.
fn stash_write() -> RunOptions {
    stash_env().rewrites_files()
}

/// The stashes, newest first.
pub async fn list_stashes(repo: &Repo) -> Result<Vec<Stash>> {
    let output = repo
        .read(["stash", "list", "-z", STASH_LIST_FORMAT])
        .await?;
    Ok(parse_stash_list(&output))
}

/// Stashes every uncommitted change, staged or not, untracked files included, so none are left.
/// Git refuses before the first commit, and with conflicts, saying so. With nothing to stash, it
/// does nothing.
pub async fn push_stash(repo: &Repo) -> Result<()> {
    stash_changes(&repo.writer(), None).await
}

/// `push_stash` through `run` (e.g. a command of `repo.exclusive`), with `message` if given.
pub async fn stash_changes(run: &GitCommand, message: Option<&str>) -> Result<()> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        args.extend(["--message", message]);
    }
    // Not `--quiet`, which keeps git from saying why it refused, too.
    run.run_with(args, stash_write()).await?;
    Ok(())
}

/// The files the stash `sha` changed compared to its base, untracked ones included.
pub async fn get_stash_files(repo: &Repo, sha: &str) -> Result<Vec<ChangedFile>> {
    // Diff options of its own, so `stash.showStat` and `stash.showPatch` don't add any.
    let output = repo
        .read_with(
            [
                "stash",
                "show",
                "--include-untracked",
                "-M",
                "--raw",
                "--numstat",
                "-z",
                sha,
            ],
            stash_env(),
        )
        .await?;
    Ok(parse_diff(&output))
}

/// The patch of one file the stash `sha` changed, compared to its base like `get_stash_files`. A
/// stash is a merge of what was staged into its base, with the working tree's files, so the
/// tracked ones are compared to its first parent like a commit's. The untracked files it stashed
/// are in a commit of their own, its third parent, without one.
pub async fn get_stash_file_patch(repo: &Repo, sha: &str, file: &FileInput) -> Result<String> {
    let run = repo.git();
    let commit = Sha::parse(sha)?;
    let third = format!("{sha}^3");
    let (tracked, untracked) = futures::try_join!(
        get_commit_file_patch(repo, &commit, file),
        resolve_ref(&run, &third)
    )?;
    let Some(untracked) = untracked else {
        return Ok(tracked);
    };
    if tracked.is_empty() {
        let args = [
            "diff-tree",
            "-p",
            "-r",
            "--root",
            "--full-index",
            "--no-commit-id",
            &untracked,
            "--",
            &file.path,
        ];
        return read_patch(&run, args).await;
    }
    // In the stash's own diff, which its files are listed from, an untracked file can pair with a
    // tracked one that was deleted, as a rename, or take the place of one deleted from the index.
    // Only that diff has those, with both kinds of files in it.
    let paired = match file.orig_path.as_deref() {
        Some(orig_path) if !orig_path.is_empty() => {
            !tracked.contains(&format!("\nrename to {}\n", file.path))
        }
        _ => tracked.starts_with("deleted file mode ") || tracked.contains("\ndeleted file mode "),
    };
    if !paired {
        return Ok(tracked);
    }
    Ok(stashed_file_patch(repo, sha, file)
        .await?
        .unwrap_or(tracked))
}

/// `file`'s patch in all of the stash `sha`'s, untracked files included, as `git stash show` gives
/// it, which takes no paths; `None` if it isn't there, or git quoted its name.
async fn stashed_file_patch(repo: &Repo, sha: &str, file: &FileInput) -> Result<Option<String>> {
    let mut args = vec![
        "stash",
        "show",
        "-p",
        "--include-untracked",
        "-M",
        "--full-index",
    ];
    args.extend(PORCELAIN_DIFF);
    args.push(sha);
    let patch = read_patch(&repo.git(), args).await?;
    let from = file.orig_path.as_deref().unwrap_or(&file.path);
    let header = format!("diff --git a/{from} b/{}\n", file.path);
    // Where the header is, from the newline before it, the patch's first line or not.
    let start = if patch.starts_with(&header) {
        Some(0)
    } else {
        patch.find(&format!("\n{header}")).map(|at| at + 1)
    };
    let Some(start) = start else {
        return Ok(None);
    };
    Ok(Some(match patch[start..].find("\ndiff --git ") {
        Some(end) => patch[start..start + end + 1].to_owned(),
        None => patch[start..].to_owned(),
    }))
}

/// Puts the stash `sha` back and drops it, the newest or an older one. What was staged when it was
/// stashed is staged again, if that can be done safely (see `pop_ref`). A pop that conflicts is
/// left to resolve, and the stash kept, as git does.
pub async fn pop_stash(repo: &Repo, sha: &str) -> Result<()> {
    // One write, so no other stash of Gitto's can move it between finding and popping it.
    repo.exclusive(|run| async move {
        let stash = stash_ref(&run, sha, "popped").await?;
        pop_ref(&run, &stash, Some(sha)).await
    })
    .await
}

/// Pops the newest stash through `run` (a command of `repo.exclusive`), as `pop_stash` does,
/// without checking which one it is.
pub async fn pop_newest(run: &GitCommand) -> Result<()> {
    pop_ref(run, "stash@{0}", None).await
}

/// "Dropped stash@{1} (<sha>)", as git says it dropped a stash.
static DROPPED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^Dropped .+ \(([0-9a-f]+)\)$").expect("a valid regex"));

/// Drops the stash `sha`, the newest or an older one, throwing its changes away. Should another
/// have been dropped instead, as git outside Gitto pushed or dropped one meanwhile, it's put back.
pub async fn drop_stash(repo: &Repo, sha: &str) -> Result<()> {
    // One write, so no other stash of Gitto's can move it between finding and dropping it.
    repo.exclusive(|run| async move {
        let stash = stash_ref(&run, sha, "deleted").await?;
        // Not `--quiet`: git names the stash it dropped, as "Dropped stash@{1} (<sha>)".
        let output = run.run_with(["stash", "drop", &stash], stash_env()).await?;
        let dropped = DROPPED
            .captures(&output)
            .and_then(|captures| captures.get(1))
            .map(|dropped| dropped.as_str().to_owned());
        if dropped.as_deref() == Some(sha) {
            return Ok(());
        }
        if let Some(dropped) = dropped {
            // Its message, as the stash's commit has it, to keep it as it was named.
            let message = run
                .run(["show", "--no-patch", "--format=%s", &dropped])
                .await?;
            run.run_with(
                ["stash", "store", "--message", message.trim(), &dropped],
                stash_env(),
            )
            .await?;
        }
        Err(Error::RepositoryChanged(
            "The stashes changed as the stash was being deleted, so it wasn't. Another one was dropped instead, and has been put back as the newest.".to_owned(),
        ))
    })
    .await
}

/// The stash `sha` as git names it among the others, e.g. `stash@{2}`, through `run`; fails if
/// it's gone, saying it wasn't `done` (e.g. "popped"). The first, should it be in the list more
/// than once: the same commit, so the same changes, either way.
async fn stash_ref(run: &GitCommand, sha: &str, done: &str) -> Result<String> {
    let output = run.run(["stash", "list", "-z", STASH_LIST_FORMAT]).await?;
    match parse_stash_list(&output)
        .iter()
        .position(|stash| stash.sha == sha)
    {
        Some(index) => Ok(format!("stash@{{{index}}}")),
        None => Err(stash_gone(done)),
    }
}

/// Says the stash is gone, so it wasn't `done` (e.g. "popped").
fn stash_gone(done: &str) -> Error {
    Error::RepositoryChanged(format!(
        "The stash is gone: the stashes changed before it could be {done}."
    ))
}

/// Pops the stash `stash` (e.g. `stash@{1}`) through `run`, as `pop_stash` does; as long as it's
/// still the stash `sha`, if given, which git outside Gitto can change.
async fn pop_ref(run: &GitCommand, stash: &str, sha: Option<&str>) -> Result<()> {
    let (conflicted, staged) = futures::try_join!(has_conflicts(run), has_staged_changes(run))?;
    // Checked last thing before popping it.
    if let Some(sha) = sha
        && resolve_ref(run, stash).await?.as_deref() != Some(sha)
    {
        return Err(stash_gone("popped"));
    }
    let popped: Result<()> = async {
        // Putting back what was staged (`--index`) is only tried when nothing is staged now: when
        // something is, a pop that fails partway can unstage it, and leave half the stash
        // applied. Without staged changes, it either works, fails like a plain pop, or fails
        // before changing anything when the staged changes don't apply, and a plain pop follows.
        if !staged && pop_index(run, stash).await? {
            return Ok(());
        }
        run.run_with(["stash", "pop", "--quiet", stash], stash_write())
            .await?;
        Ok(())
    }
    .await;
    match popped {
        // Git won't pop over conflicts that were already there, and says so.
        Err(error) if error.as_git().is_some() && !conflicted => {
            if has_conflicts(run).await? {
                return Err(Error::StashConflict(
                    "Popping the stash caused conflicts. Resolve them. The stash was kept, as git does when a pop conflicts.".to_owned(),
                ));
            }
            Err(error)
        }
        popped => popped,
    }
}

/// "Index was not unstashed.", which git says when a pop with `--index` was refused.
static INDEX_NOT_UNSTASHED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^Index was not unstashed\.\n?").expect("a valid regex"));

/// "conflicts in index", which git says when what was staged doesn't apply to the index.
static CONFLICTS_IN_INDEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)conflicts in index").expect("a valid regex"));

/// Pops the stash `stash` with what was staged staged again; `false`, having changed nothing, when
/// that won't apply to the index.
async fn pop_index(run: &GitCommand, stash: &str) -> Result<bool> {
    let error = match run
        .run_with(["stash", "pop", "--index", "--quiet", stash], stash_write())
        .await
    {
        Ok(_) => return Ok(true),
        Err(error) => error,
    };
    let Some(git) = error.as_git() else {
        return Err(error);
    };
    if CONFLICTS_IN_INDEX.is_match(&git.stderr) {
        return Ok(false);
    }
    // Said when the pop was refused, e.g. as it'd overwrite local changes: a plain pop wouldn't
    // have restaged anything either, so that's not news.
    let message = INDEX_NOT_UNSTASHED.replace(&git.message, "");
    let message = message.trim();
    if message.is_empty() || message == git.message {
        return Err(error);
    }
    Err(Error::Git(git.with_message(message)))
}

/// Whether anything is staged; fails if git couldn't tell.
async fn has_staged_changes(run: &GitCommand) -> Result<bool> {
    // Exits with 1 when something is.
    Ok(!succeeds(run, ["diff", "--cached", "--quiet"]).await?)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::core::repo::{How, Observed};
    use crate::schema::FileStatus;
    use crate::test_support::{Fixture, git};

    fn code(letter: Option<char>) -> Option<&'static str> {
        Some(match letter? {
            'M' => "modified",
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            'C' => "copied",
            'T' => "typechange",
            _ => return None,
        })
    }

    type Row = (String, Option<&'static str>, Option<&'static str>);

    /// Each changed file, with how it changed in the index and the working tree, as the status
    /// feature's `parseStatus` reads `STATUS_ARGS`. A stand-in for it until it's ported.
    async fn status_files(repo: &Repo) -> Vec<Row> {
        let output = repo
            .read([
                "status",
                "--porcelain=v2",
                "-z",
                "--branch",
                "--untracked-files=all",
            ])
            .await
            .unwrap();
        let mut records = output.split('\0');
        let mut files = Vec::new();
        while let Some(record) = records.next() {
            let fields = |count| record.splitn(count, ' ').collect::<Vec<_>>();
            let (path, xy) = match record.chars().next() {
                Some('1') => {
                    let fields = fields(9);
                    (fields[8], fields[1])
                }
                Some('2') => {
                    let fields = fields(10);
                    records.next();
                    (fields[9], fields[1])
                }
                Some('u') => {
                    let fields = fields(11);
                    let conflicted = Some("conflicted");
                    files.push((fields[10].to_owned(), conflicted, conflicted));
                    continue;
                }
                Some('?') => {
                    files.push((record[2..].to_owned(), None, Some("untracked")));
                    continue;
                }
                _ => continue,
            };
            let mut xy = xy.chars();
            files.push((path.to_owned(), code(xy.next()), code(xy.next())));
        }
        files
    }

    fn rows(rows: &[(&str, Option<&'static str>, Option<&'static str>)]) -> Vec<Row> {
        rows.iter()
            .map(|(path, staged, unstaged)| (path.to_string(), *staged, *unstaged))
            .collect()
    }

    /// A repository with `a.txt` and `b.txt` committed, which `repos` opens as `name`.
    fn create_committed_repo(fx: &Fixture, name: &str) -> PathBuf {
        let path = fx.create_repo(name);
        fs::write(path.join("a.txt"), "a\n").unwrap();
        fs::write(path.join("b.txt"), "b\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        path
    }

    /// `repo`, with `meanwhile` run once its stashes have been listed in a write, as git outside
    /// Gitto could, which a write doesn't keep out.
    fn listed_then(repo: &Repo, meanwhile: impl Fn() + Send + Sync + 'static) -> Repo {
        repo.observed(move |observed| {
            if let Observed::Ran {
                how: How::Exclusive,
                args,
            } = observed
                && args.len() > 1
                && args[0] == "stash"
                && args[1] == "list"
            {
                meanwhile();
            }
        })
    }

    /// Pushes a stash of the changes in `path` from outside Gitto, with `b.txt` changed.
    fn stash_outside(path: &Path) {
        fs::write(path.join("b.txt"), "outside\n").unwrap();
        git(path, ["stash", "push", "-q", "-m", "outside"]);
    }

    /// The newest stash's SHA.
    async fn newest(repo: &Repo) -> String {
        let stashes = list_stashes(repo).await.unwrap();
        stashes.first().expect("expected a stash").sha.clone()
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn file(path: &str, orig_path: Option<&str>) -> FileInput {
        FileInput {
            path: path.to_owned(),
            orig_path: orig_path.map(str::to_owned),
        }
    }

    #[test]
    fn lists_none_before_anything_is_stashed() {
        let fx = Fixture::new();
        create_committed_repo(&fx, "none");
        smol::block_on(async {
            let repo = fx.repos.open("none").await.unwrap();
            assert_eq!(list_stashes(&repo).await.unwrap(), []);
        });
    }

    #[test]
    fn stashes_every_change_and_pops_them_back_as_they_were() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "round-trip");
        fs::write(path.join("a.txt"), "staged\n").unwrap();
        git(&path, ["add", "a.txt"]);
        fs::write(path.join("b.txt"), "unstaged\n").unwrap();
        fs::write(path.join("new file.txt"), "new\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("round-trip").await.unwrap();
            let before = status_files(&repo).await;

            push_stash(&repo).await.unwrap();
            assert_eq!(status_files(&repo).await, rows(&[]));
            let stashes = list_stashes(&repo).await.unwrap();
            assert_eq!(stashes.len(), 1);
            let stash = &stashes[0];
            assert_eq!(stash.sha, git(&path, ["rev-parse", "refs/stash"]));
            assert_eq!(stash.base, git(&path, ["rev-parse", "HEAD"]));
            let message = Regex::new("^WIP on main: [0-9a-f]+ first$").unwrap();
            assert!(message.is_match(&stash.message), "{}", stash.message);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            assert!((stash.created_at - now).abs() < 60_000);
            // What it changed, staged or not, and the untracked files.
            let changed = |path: &str, status, additions, deletions| ChangedFile {
                path: path.to_owned(),
                status,
                orig_path: None,
                additions: Some(additions),
                deletions: Some(deletions),
            };
            assert_eq!(
                get_stash_files(&repo, &stash.sha).await.unwrap(),
                [
                    changed("a.txt", FileStatus::Modified, 1, 1),
                    changed("b.txt", FileStatus::Modified, 1, 1),
                    changed("new file.txt", FileStatus::Added, 1, 0),
                ]
            );

            pop_stash(&repo, &stash.sha).await.unwrap();
            // What was staged is staged again.
            assert_eq!(status_files(&repo).await, before);
            assert_eq!(list_stashes(&repo).await.unwrap(), []);
        });
    }

    #[test]
    fn lists_the_newest_first_and_pops_an_older_one_by_its_sha() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "several");
        fs::write(path.join("a.txt"), "older\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "older"]);
        fs::write(path.join("b.txt"), "newer\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "newer: with a colon"]);
        smol::block_on(async {
            let repo = fx.repos.open("several").await.unwrap();

            let stashes = list_stashes(&repo).await.unwrap();
            let messages: Vec<&str> = stashes.iter().map(|stash| &*stash.message).collect();
            assert_eq!(messages, ["On main: newer: with a colon", "On main: older"]);

            pop_stash(&repo, &stashes[1].sha).await.unwrap();
            assert_eq!(list_stashes(&repo).await.unwrap(), [stashes[0].clone()]);
            assert_eq!(read(&path.join("a.txt")), "older\n");
            assert_eq!(read(&path.join("b.txt")), "b\n");

            // Gone now.
            let error = pop_stash(&repo, &stashes[1].sha).await.unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            assert_eq!(list_stashes(&repo).await.unwrap(), [stashes[0].clone()]);
        });
    }

    #[test]
    fn drops_a_stash_by_its_sha_leaving_the_others_and_the_changes() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "drop");
        fs::write(path.join("a.txt"), "older\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "older"]);
        fs::write(path.join("a.txt"), "middle\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "middle"]);
        fs::write(path.join("a.txt"), "newer\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "newer"]);
        fs::write(path.join("b.txt"), "local\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("drop").await.unwrap();
            let stashes = list_stashes(&repo).await.unwrap();
            let [newer, middle, older] = [&stashes[0], &stashes[1], &stashes[2]];

            drop_stash(&repo, &middle.sha).await.unwrap();
            assert_eq!(
                list_stashes(&repo).await.unwrap(),
                [newer.clone(), older.clone()]
            );
            assert_eq!(
                status_files(&repo).await,
                rows(&[("b.txt", None, Some("modified"))])
            );

            drop_stash(&repo, &newer.sha).await.unwrap();
            assert_eq!(
                list_stashes(&repo).await.unwrap(),
                std::slice::from_ref(older)
            );

            let error = drop_stash(&repo, &middle.sha).await.unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            assert_eq!(
                list_stashes(&repo).await.unwrap(),
                std::slice::from_ref(older)
            );
        });
    }

    #[test]
    fn pops_nothing_when_git_outside_gitto_moves_the_stash() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "pop-moved");
        fs::write(path.join("a.txt"), "older\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "older"]);
        fs::write(path.join("a.txt"), "newer\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "newer"]);
        smol::block_on(async {
            let repo = fx.repos.open("pop-moved").await.unwrap();
            let stashes = list_stashes(&repo).await.unwrap();
            let outside = path.clone();
            let moving = listed_then(&repo, move || stash_outside(&outside));

            let error = pop_stash(&moving, &stashes[1].sha).await.unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            assert_eq!(list_stashes(&repo).await.unwrap()[1..], stashes[..]);
            assert_eq!(status_files(&repo).await, rows(&[]));
        });
    }

    #[test]
    fn puts_back_another_stash_dropped_instead_when_git_outside_gitto_moves_the_stash() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "drop-moved");
        fs::write(path.join("a.txt"), "older\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "older"]);
        fs::write(path.join("a.txt"), "newer\n").unwrap();
        git(&path, ["stash", "push", "-q", "-m", "newer"]);
        smol::block_on(async {
            let repo = fx.repos.open("drop-moved").await.unwrap();
            let stashes = list_stashes(&repo).await.unwrap();
            let [newer, older] = [&stashes[0], &stashes[1]];
            let outside = path.clone();
            let moving = listed_then(&repo, move || stash_outside(&outside));

            let error = drop_stash(&moving, &older.sha).await.unwrap_err();
            assert!(matches!(error, Error::RepositoryChanged(_)), "{error:?}");
            let after = list_stashes(&repo).await.unwrap();
            assert_eq!(after.len(), 3);
            assert_eq!(
                (&after[0].sha, &after[0].message),
                (&newer.sha, &newer.message)
            );
            assert_eq!(after[1].message, "On main: outside");
            assert_eq!(
                (&after[2].sha, &after[2].message),
                (&older.sha, &older.message)
            );
        });
    }

    #[test]
    fn does_nothing_when_theres_nothing_to_stash() {
        let fx = Fixture::new();
        create_committed_repo(&fx, "clean");
        smol::block_on(async {
            let repo = fx.repos.open("clean").await.unwrap();
            push_stash(&repo).await.unwrap();
            assert_eq!(list_stashes(&repo).await.unwrap(), []);
        });
    }

    #[test]
    fn pops_without_restaging_when_something_is_staged_already_keeping_that_staged() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "staged-already");
        fs::write(path.join("a.txt"), "stashed\n").unwrap();
        git(&path, ["add", "a.txt"]);
        smol::block_on(async {
            let repo = fx.repos.open("staged-already").await.unwrap();
            push_stash(&repo).await.unwrap();
            fs::write(path.join("other.txt"), "other\n").unwrap();
            git(&path, ["add", "other.txt"]);

            pop_stash(&repo, &newest(&repo).await).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                rows(&[
                    ("a.txt", None, Some("modified")),
                    ("other.txt", Some("added"), None),
                ])
            );
            assert_eq!(list_stashes(&repo).await.unwrap(), []);
        });
    }

    #[test]
    fn pops_without_restaging_when_what_was_staged_no_longer_applies() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "index-moved");
        fs::write(path.join("a.txt"), "1\n2\n3\n4\n5\n").unwrap();
        git(&path, ["commit", "-qam", "lines"]);
        fs::write(path.join("a.txt"), "1\nstashed\n3\n4\n5\n").unwrap();
        git(&path, ["add", "a.txt"]);
        fs::write(path.join("new.txt"), "new\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("index-moved").await.unwrap();
            push_stash(&repo).await.unwrap();
            // Changes a line next to the stashed one: the staged diff no longer applies, but the
            // stash still merges cleanly.
            fs::write(path.join("a.txt"), "1\n2\n3\ncommitted\n5\n").unwrap();
            git(&path, ["commit", "-qam", "change a"]);

            pop_stash(&repo, &newest(&repo).await).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                rows(&[
                    ("a.txt", None, Some("modified")),
                    ("new.txt", None, Some("untracked")),
                ])
            );
            assert_eq!(read(&path.join("a.txt")), "1\nstashed\n3\ncommitted\n5\n");
            assert_eq!(list_stashes(&repo).await.unwrap(), []);
        });
    }

    #[test]
    fn leaves_a_pop_that_conflicts_to_resolve_and_keeps_the_stash() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "conflict");
        fs::write(path.join("a.txt"), "stashed\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("conflict").await.unwrap();
            push_stash(&repo).await.unwrap();
            fs::write(path.join("a.txt"), "committed\n").unwrap();
            git(&path, ["commit", "-qam", "change a"]);
            let sha = newest(&repo).await;

            let error = pop_stash(&repo, &sha).await.unwrap_err();
            assert!(matches!(error, Error::StashConflict(_)), "{error:?}");
            assert_eq!(
                status_files(&repo).await,
                rows(&[("a.txt", Some("conflicted"), Some("conflicted"))])
            );
            assert_eq!(newest(&repo).await, sha);

            // Git refuses to pop over the conflicts, which aren't the pop's.
            let error = pop_stash(&repo, &sha).await.unwrap_err();
            assert!(!matches!(error, Error::StashConflict(_)), "{error:?}");
        });
    }

    #[test]
    fn says_why_a_pop_that_would_overwrite_local_changes_was_refused_and_keeps_the_stash() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "overwrite");
        fs::write(path.join("a.txt"), "stashed\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("overwrite").await.unwrap();
            push_stash(&repo).await.unwrap();
            fs::write(path.join("a.txt"), "local\n").unwrap();
            let sha = newest(&repo).await;

            let message = pop_stash(&repo, &sha).await.unwrap_err().to_string();
            let overwritten = Regex::new("local changes .* would be overwritten").unwrap();
            assert!(overwritten.is_match(&message), "{message}");
            assert!(!message.contains("Index was not unstashed"), "{message}");
            assert_eq!(read(&path.join("a.txt")), "local\n");
            assert_eq!(newest(&repo).await, sha);
        });
    }

    #[test]
    fn says_why_changes_cant_be_stashed_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("unborn");
        fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&path, ["add", "a.txt"]);
        smol::block_on(async {
            let repo = fx.repos.open("unborn").await.unwrap();
            let message = push_stash(&repo).await.unwrap_err().to_string();
            assert!(message.contains("initial commit"), "{message}");
        });
    }

    // getStashFilePatch

    #[test]
    fn shows_a_stashed_files_changes_compared_to_the_stashs_base_untracked_ones_as_added() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "stash-patches");
        smol::block_on(async {
            let repo = fx.repos.open("stash-patches").await.unwrap();
            fs::write(path.join("a.txt"), "a\nmore\n").unwrap();
            git(&path, ["mv", "b.txt", "c.txt"]);
            fs::write(path.join("new.txt"), "new\n").unwrap();
            push_stash(&repo).await.unwrap();
            let sha = newest(&repo).await;

            let patch = get_stash_file_patch(&repo, &sha, &file("a.txt", None))
                .await
                .unwrap();
            assert!(patch.contains("@@ -1 +1,2 @@\n a\n+more\n"), "{patch}");
            let patch = get_stash_file_patch(&repo, &sha, &file("c.txt", Some("b.txt")))
                .await
                .unwrap();
            assert!(
                patch.contains("rename from b.txt\nrename to c.txt\n"),
                "{patch}"
            );
            let untracked = get_stash_file_patch(&repo, &sha, &file("new.txt", None))
                .await
                .unwrap();
            assert!(
                untracked.starts_with("diff --git a/new.txt b/new.txt\nnew file mode 100644\n"),
                "{untracked}"
            );
            assert!(untracked.contains("@@ -0,0 +1 @@\n+new\n"), "{untracked}");
        });
    }

    #[test]
    fn is_empty_for_a_file_the_stash_didnt_change_with_or_without_untracked_files() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "stash-unchanged");
        smol::block_on(async {
            let repo = fx.repos.open("stash-unchanged").await.unwrap();
            fs::write(path.join("a.txt"), "changed\n").unwrap();
            git(&path, ["stash", "push", "-q"]);
            let sha = newest(&repo).await;
            assert_eq!(
                get_stash_file_patch(&repo, &sha, &file("b.txt", None))
                    .await
                    .unwrap(),
                ""
            );
        });
    }

    #[test]
    fn shows_a_deleted_file_paired_with_an_untracked_one_as_the_rename_its_listed_as() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "stash-untracked-rename");
        smol::block_on(async {
            let repo = fx.repos.open("stash-untracked-rename").await.unwrap();
            fs::write(path.join("old.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
            git(&path, ["add", "old.txt"]);
            git(&path, ["commit", "-qm", "old"]);
            git(&path, ["config", "diff.noprefix", "true"]);
            git(&path, ["rm", "-q", "old.txt"]);
            fs::write(path.join("new.txt"), "one\ntwo\nthree\nfour\nfive\nsix\n").unwrap();
            push_stash(&repo).await.unwrap();
            let sha = newest(&repo).await;

            let renamed = get_stash_files(&repo, &sha)
                .await
                .unwrap()
                .into_iter()
                .find(|file| file.path == "new.txt")
                .unwrap();
            assert_eq!(renamed.status, FileStatus::Renamed);
            assert_eq!(renamed.orig_path.as_deref(), Some("old.txt"));
            let patch = get_stash_file_patch(
                &repo,
                &sha,
                &file(&renamed.path, renamed.orig_path.as_deref()),
            )
            .await
            .unwrap();
            assert!(
                patch.starts_with("diff --git a/old.txt b/new.txt\n"),
                "{patch}"
            );
            assert!(
                patch.contains("rename from old.txt\nrename to new.txt\n"),
                "{patch}"
            );
            assert!(patch.contains(" five\n+six\n"), "{patch}");
            assert!(!patch.contains("diff --git a/a.txt"), "{patch}");
        });
    }
}
