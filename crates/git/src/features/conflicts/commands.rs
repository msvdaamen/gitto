//! Reading a conflicted file's sides and what's in the working tree, and resolving it: by keeping
//! a side whole, or as it is in the working tree. Ported from `commands.ts`.

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::future::try_join_all;

use super::markers::{BINARY_CHECK_BYTES, ConflictCounter, is_binary};
use super::schema::{Conflict, ConflictSide, ConflictSides, ConflictText, Side};
use crate::core::errors::{DiskChange, Error, Result};
use crate::core::repo::{GitCommand, Repo};
use crate::core::runner::{RunOptions, lock};
use crate::features::diff::limits::MAX_BLOB_BYTES;
use crate::features::diff::working_tree::{
    decode_utf8, read_working_tree_bytes, scan_working_tree_file,
};

/// A submodule's mode in the index: its commit, rather than a file.
const SUBMODULE: &str = "160000";

/// Refuses an empty path, as the contract's `z.string().min(1)` did.
fn check_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(Error::InvalidInput("Not a path.".to_owned()));
    }
    Ok(())
}

/// The sides of the conflicted file at `path`, as `git ls-files -u` lists them: stage 1 is the
/// common ancestor's, 2 ours, 3 theirs. All `None` once it isn't conflicted.
pub async fn read_sides(run: &GitCommand, path: &str) -> Result<ConflictSides> {
    let mut sides = ConflictSides::default();
    // `<mode> <object> <stage>\t<path>`; a folder's would list the files in it, which aren't it.
    let output = run.run(["ls-files", "-u", "-z", "--", path]).await?;
    for entry in output.split('\0') {
        let Some((info, entry_path)) = entry.split_once('\t') else {
            continue;
        };
        if entry_path != path {
            continue;
        }
        let mut fields = info.split(' ');
        let mode = fields.next().unwrap_or("").to_owned();
        let oid = fields.next().unwrap_or("").to_owned();
        let side = Some(ConflictSide { mode, oid });
        match fields.next() {
            Some("1") => sides.base = side,
            Some("2") => sides.ours = side,
            Some("3") => sides.theirs = side,
            _ => {}
        }
    }
    Ok(sides)
}

/// The conflicted paths in the index, e.g. after a merge stopped at conflicts, in git's order.
async fn unmerged_paths(run: &GitCommand) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    // Each path is listed once per side it has.
    for entry in run.run(["ls-files", "-u", "-z"]).await?.split('\0') {
        if let Some((_, path)) = entry.split_once('\t')
            && seen.insert(path.to_owned())
        {
            paths.push(path.to_owned());
        }
    }
    Ok(paths)
}

/// The conflicted file at `path`: its sides, and the file in the working tree, as text to resolve
/// its conflicts in if it's text, with its version, which writing it checks it still has.
pub async fn get_conflict(repo: &Repo, path: &str) -> Result<Conflict> {
    check_path(path)?;
    let run = repo.git();
    let (sides, file) = futures::try_join!(read_sides(&run, path), read_text(repo, path))?;
    Ok(Conflict {
        base: sides.base,
        ours: sides.ours,
        theirs: sides.theirs,
        text: file.text,
        version: file.version,
        binary: file.binary,
        unreadable: file.unreadable,
    })
}

/// What `read_text` has of the file in the working tree: `Conflict`'s fields but its sides.
struct FileText {
    text: Option<ConflictText>,
    version: Option<String>,
    binary: bool,
    unreadable: Option<String>,
}

impl FileText {
    fn none(version: Option<String>, unreadable: Option<String>) -> Self {
        FileText {
            text: None,
            version,
            binary: false,
            unreadable,
        }
    }
}

/// What's in the working tree at `path`, as `get_conflict` has it. Not what a symbolic link leads
/// to, which is no part of the conflict, and can be outside the repository: a link's conflict is
/// resolved by keeping a side.
async fn read_text(repo: &Repo, path: &str) -> Result<FileText> {
    if is_link(repo, path).await {
        return Ok(FileText::none(None, None));
    }
    let file = match read_working_tree_bytes(repo, path, MAX_BLOB_BYTES).await {
        Ok(file) => file,
        Err(Error::FileTooLarge { .. }) => {
            // Still with a version, which keeping a side checks first: when it last changed,
            // rather than its bytes, which this is read again for whenever the working tree
            // changes.
            let version = stamp_of(repo, path)
                .await
                .map(|stamp| format!("{STAMPED}{stamp}"));
            return Ok(FileText::none(
                version,
                Some("This file is too large to show.".to_owned()),
            ));
        }
        // In a folder that's a link out of the repository, say.
        Err(error @ Error::OutsideRepository { .. }) => {
            return Ok(FileText::none(None, Some(error.to_string())));
        }
        Err(error) => return Err(error),
    };
    let Some(file) = file else {
        return Ok(FileText::none(None, None));
    };
    let version = file.version;
    if is_binary(&file.bytes) {
        return Ok(FileText {
            text: None,
            version: Some(version),
            binary: true,
            unreadable: Some("This file is binary.".to_owned()),
        });
    }
    match decode_utf8(&file.bytes) {
        Ok(contents) => Ok(FileText {
            text: Some(ConflictText {
                contents,
                version: version.clone(),
            }),
            version: Some(version),
            binary: false,
            unreadable: None,
        }),
        Err(error @ Error::NotUtf8) => Ok(FileText::none(Some(version), Some(error.to_string()))),
        Err(error) => Err(error),
    }
}

/// What the user saw of a conflict, which resolving it checks is still so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownConflict {
    pub sides: ConflictSides,
    /// The version of the file in the working tree, if there was one to check; see `get_conflict`.
    pub version: Option<String>,
}

/// Resolves the conflict in the file at `path` by keeping one side whole, `side`, and marks it
/// resolved: the file as that side has it, or deleted if it deleted it. A submodule's commit is
/// only put in the index; its folder is left as it is, to update when the user's ready. Fails
/// with `Error::ConflictChanged` if the conflict isn't the one `shown` any more, and with
/// `Error::FileChangedOnDisk` if the file changed on disk since, which keeping a side would lose.
pub async fn keep_side(repo: &Repo, path: &str, side: Side, shown: &ShownConflict) -> Result<()> {
    check_path(path)?;
    repo.exclusive(|run| async move {
        let sides = read_sides(&run, path).await?;
        if !same_sides(&sides, &shown.sides) {
            return Err(Error::ConflictChanged {
                path: path.to_owned(),
            });
        }
        if let Some(version) = &shown.version {
            check_version(repo, path, version).await?;
        }
        let kept = match side {
            Side::Ours => &sides.ours,
            Side::Theirs => &sides.theirs,
        };
        match kept {
            None => {
                run.run_with(
                    ["rm", "--quiet", "--", path],
                    RunOptions::default().rewrites_files(),
                )
                .await?;
            }
            Some(kept) if kept.mode == SUBMODULE => {
                let info = format!("{},{},{path}", kept.mode, kept.oid);
                run.run(["update-index", "--cacheinfo", &info]).await?;
            }
            Some(_) => {
                let side = format!("--{}", side.as_str());
                run.run_with(
                    ["checkout", &side, "--", path],
                    RunOptions::default().rewrites_files(),
                )
                .await?;
                run.run(["add", "--", path]).await?;
            }
        }
        Ok(())
    })
    .await
}

/// Starts a version that's when a file last changed (see `stamp_of`), not what's in it.
const STAMPED: &str = "stamp:";

/// Fails with `Error::FileChangedOnDisk` if the file at `path` in the working tree isn't at
/// `version` any more, or is gone. Read a piece at a time, however large it is.
async fn check_version(repo: &Repo, path: &str, version: &str) -> Result<()> {
    let now = if version.starts_with(STAMPED) {
        stamp_of(repo, path)
            .await
            .map(|stamp| format!("{STAMPED}{stamp}"))
    } else {
        scan_working_tree_file(repo, path, |_| {}, u64::MAX).await?
    };
    if now.as_deref() != Some(version) {
        return Err(Error::FileChangedOnDisk {
            path: path.to_owned(),
            how: if now.is_some() {
                DiskChange::Changed
            } else {
                DiskChange::Deleted
            },
        });
    }
    Ok(())
}

fn same_sides(a: &ConflictSides, b: &ConflictSides) -> bool {
    same_side(&a.base, &b.base) && same_side(&a.ours, &b.ours) && same_side(&a.theirs, &b.theirs)
}

fn same_side(a: &Option<ConflictSide>, b: &Option<ConflictSide>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.mode == b.mode && a.oid == b.oid,
        (None, None) => true,
        _ => false,
    }
}

/// Marks the conflicted file at `path` resolved, as it is in the working tree: stages it, or its
/// deletion if it's gone. Fails with `Error::ConflictMarkers` if it still has conflict markers,
/// unless `with_markers`: the user said they belong in it, like a test's fixture of them. Fails
/// with `Error::FileChangedOnDisk` if it isn't at `version` (when given), the version the user
/// saw. Does nothing if it isn't conflicted any more, e.g. as it was staged in a terminal.
pub async fn mark_resolved(
    repo: &Repo,
    path: &str,
    version: Option<&str>,
    with_markers: bool,
) -> Result<()> {
    check_path(path)?;
    repo.exclusive(|run| async move {
        let sides = read_sides(&run, path).await?;
        if sides.base.is_none() && sides.ours.is_none() && sides.theirs.is_none() {
            return Ok(());
        }
        if let Some(version) = version {
            check_version(repo, path, version).await?;
        }
        if !with_markers {
            refuse_conflict_markers(repo, &run, Some(&[path.to_owned()])).await?;
        }
        run.run(["add", "--all", "--", path]).await?;
        Ok(())
    })
    .await
}

/// Fails with `Error::ConflictMarkers` if staging `paths` (all of the working tree if `None`)
/// would mark a conflicted file resolved while it still has conflict markers: git would stage them
/// as they are. Through `run`, a command of the `repo.exclusive` that stages them.
pub async fn refuse_conflict_markers(
    repo: &Repo,
    run: &GitCommand,
    paths: Option<&[String]>,
) -> Result<()> {
    let unmerged = unmerged_paths(run).await?;
    if unmerged.is_empty() {
        return Ok(());
    }
    // A folder's path stages the files in it.
    let staged: Vec<String> = match paths {
        Some(paths) => unmerged
            .into_iter()
            .filter(|path| {
                paths.iter().any(|given| {
                    path == given
                        || path
                            .strip_prefix(given.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            })
            .collect(),
        None => unmerged,
    };
    // However large it is: only one that's left out could be staged with its markers.
    let marked = read_at_once(&staged, |path| async move {
        let count = conflicts_in(repo, path, u64::MAX).await?;
        Ok::<_, Error>(count.unwrap_or(0) > 0)
    })
    .await?;
    let left: Vec<String> = staged
        .iter()
        .zip(marked)
        .filter(|(_, marked)| *marked)
        .map(|(path, _)| path.clone())
        .collect();
    if !left.is_empty() {
        return Err(Error::ConflictMarkers { paths: left });
    }
    Ok(())
}

/// How many conflicted files are read at once, to tell whether they have markers left: a merge can
/// leave thousands conflicted, and every one of them open at the same time would run the process
/// out of file descriptors.
const READ_AT_ONCE: usize = 50;

/// `read` of each of `paths`, `READ_AT_ONCE` at a time, in order; stops at the first that fails.
async fn read_at_once<'a, T, E, F, Fut>(
    paths: &'a [String],
    read: F,
) -> std::result::Result<Vec<T>, E>
where
    F: Fn(&'a str) -> Fut,
    Fut: Future<Output = std::result::Result<T, E>>,
{
    let mut results = Vec::with_capacity(paths.len());
    for batch in paths.chunks(READ_AT_ONCE) {
        // One batch at a time, on purpose.
        results.extend(try_join_all(batch.iter().map(|path| read(path))).await?);
    }
    Ok(results)
}

/// How many conflicts the text file at `path` in the working tree has left; `None` if it isn't
/// one: it's gone, binary, larger than `max_bytes`, or a symbolic link, which git doesn't write
/// markers into, and which can lead anywhere.
async fn conflicts_in(repo: &Repo, path: &str, max_bytes: u64) -> Result<Option<usize>> {
    if is_link(repo, path).await {
        return Ok(None);
    }
    let mut counter = ConflictCounter::new();
    let mut read = 0usize;
    let mut binary = false;
    // A piece at a time: a conflicted file can be of any size.
    let scanned = scan_working_tree_file(
        repo,
        path,
        |chunk| {
            if read < BINARY_CHECK_BYTES
                && is_binary(&chunk[..chunk.len().min(BINARY_CHECK_BYTES - read)])
            {
                binary = true;
            }
            read += chunk.len();
            // Markers are ASCII, which reads the same in any of the encodings git merges as text.
            if !binary {
                counter.push(chunk);
            }
        },
        max_bytes,
    )
    .await;
    let version = match scanned {
        Ok(version) => version,
        Err(Error::FileTooLarge { .. } | Error::OutsideRepository { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    if version.is_none() || binary {
        return Ok(None);
    }
    Ok(Some(counter.end()))
}

/// Where `path` in the working tree is on disk, not following links.
fn on_disk(repo: &Repo, path: &str) -> PathBuf {
    repo.path().join(path)
}

/// Whether `path` in the working tree is a symbolic link; not if there's nothing there.
async fn is_link(repo: &Repo, path: &str) -> bool {
    async_fs::symlink_metadata(on_disk(repo, path))
        .await
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// When a file in the working tree last changed, going by what the file system says of it.
async fn stamp_of(repo: &Repo, path: &str) -> Option<String> {
    let metadata = async_fs::symlink_metadata(on_disk(repo, path)).await.ok()?;
    Some(stamp(&metadata))
}

#[cfg(unix)]
fn stamp(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!(
        "{}:{}:{}.{:09}:{}.{:09}",
        metadata.ino(),
        metadata.size(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec()
    )
}

#[cfg(not(unix))]
fn stamp(metadata: &std::fs::Metadata) -> String {
    let nanos = |time: std::io::Result<std::time::SystemTime>| {
        time.ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos())
    };
    format!(
        "{}:{}:{}",
        metadata.len(),
        nanos(metadata.modified()),
        nanos(metadata.created())
    )
}

/// What `marker_free` last found of a conflicted file: when it last changed then, and whether it
/// had markers left.
#[derive(Debug, Clone)]
struct MarkerCheck {
    stamp: String,
    free: bool,
}

/// The repository's conflicted files as `marker_free` last read them, by their paths. Only the
/// ones asked about last are kept.
#[derive(Default)]
struct MarkerChecks(Mutex<Arc<HashMap<String, MarkerCheck>>>);

/// Which of the conflicted text files at `paths` have no conflict markers left: resolved in the
/// working tree, but not marked so yet. Not one that's binary, which git leaves without markers,
/// nor one too large to read (see `MAX_BLOB_BYTES`), or gone. Asked on every change in the working
/// tree (see `get_status`), so a file is only read again once it changed since.
pub async fn marker_free(repo: &Repo, paths: &[String]) -> Vec<String> {
    let kept = repo.keep::<MarkerChecks>();
    let last = Arc::clone(&lock(&kept.0));
    let last = &*last;
    let Ok(read) = read_at_once(paths, |path| async move {
        let stamp = stamp_of(repo, path).await;
        let known = stamp
            .as_ref()
            .and_then(|stamp| last.get(path).filter(|known| known.stamp == *stamp));
        let free = match known {
            Some(known) => known.free,
            // Failing to read it is not being free of markers.
            None => matches!(conflicts_in(repo, path, MAX_BLOB_BYTES).await, Ok(Some(0))),
        };
        Ok::<_, Infallible>((stamp, free))
    })
    .await;
    let mut checks = HashMap::new();
    let mut free = Vec::new();
    for (path, (stamp, is_free)) in paths.iter().zip(read) {
        if let Some(stamp) = stamp {
            checks.insert(
                path.clone(),
                MarkerCheck {
                    stamp,
                    free: is_free,
                },
            );
        }
        if is_free {
            free.push(path.clone());
        }
    }
    *lock(&kept.0) = Arc::new(checks);
    free
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::features::diff::working_tree::{SaveExpectation, save_working_tree_file};
    use crate::features::staging;
    use crate::features::staging::commands::stage_all;
    use crate::features::status::commands::get_status;
    use crate::features::status::parse::{STATUS_ARGS, parse_status};
    use crate::schema::FileStatus;
    use crate::test_support::conflicts::{Files, Sides, files, text_conflict, try_git};
    use crate::test_support::{Fixture, git};

    type Row = (String, Option<&'static str>, Option<&'static str>);

    /// Each changed file, with how it changed in the index and the working tree.
    async fn status_files(repo: &Repo) -> Vec<Row> {
        parse_status(&repo.read(STATUS_ARGS).await.unwrap())
            .files
            .into_iter()
            .map(|file| {
                (
                    file.path,
                    file.staged.map(FileStatus::as_str),
                    file.unstaged.map(FileStatus::as_str),
                )
            })
            .collect()
    }

    fn rows(rows: &[(&str, Option<&'static str>, Option<&'static str>)]) -> Vec<Row> {
        rows.iter()
            .map(|(path, staged, unstaged)| (path.to_string(), *staged, *unstaged))
            .collect()
    }

    /// `getStatus(repo).changes.markerFree`.
    async fn status_marker_free(repo: &Repo) -> Vec<String> {
        get_status(repo).await.unwrap().changes.marker_free
    }

    /// The staging feature's `stage`, given the paths as the TS tests give them.
    async fn stage(repo: &Repo, paths: &[&str]) -> Result<()> {
        let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
        staging::commands::stage(repo, &paths).await
    }

    /// What the user saw of `conflict`, to resolve it by.
    fn shown(conflict: &Conflict) -> ShownConflict {
        ShownConflict {
            sides: conflict.sides(),
            version: conflict.version.clone(),
        }
    }

    /// The object `git` has for `spec`, e.g. `:2:f.txt` for ours of a conflicted file.
    fn oid(path: &Path, spec: &str) -> String {
        git(path, ["rev-parse", spec])
    }

    fn side(mode: &str, oid: String) -> Option<ConflictSide> {
        Some(ConflictSide {
            mode: mode.to_owned(),
            oid,
        })
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    // A text conflict.

    fn is_read_with_its_markers(style: &str, markers: &str) {
        let fx = Fixture::new();
        let name = format!("text-{style}");
        let path = fx.create_merge_conflict(&name, Some(style), &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open(&name).await.unwrap();
            // The base's marker names the commit the branches forked from.
            let contents =
                markers.replacen("BASE", &git(&path, ["rev-parse", "--short", "HEAD~"]), 1);

            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            let version = conflict.text.as_ref().unwrap().version.clone();
            assert_eq!(
                conflict,
                Conflict {
                    base: side("100644", oid(&path, ":1:f.txt")),
                    ours: side("100644", oid(&path, ":2:f.txt")),
                    theirs: side("100644", oid(&path, ":3:f.txt")),
                    text: Some(ConflictText {
                        contents,
                        version: version.clone(),
                    }),
                    version: Some(version),
                    binary: false,
                    unreadable: None,
                }
            );
            assert_eq!(status_marker_free(&repo).await, Vec::<String>::new());
        });
    }

    #[test]
    fn is_read_with_its_markers_in_the_merge_style() {
        is_read_with_its_markers(
            "merge",
            "one\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\nthree\n",
        );
    }

    #[test]
    fn is_read_with_its_markers_in_the_diff3_style() {
        is_read_with_its_markers(
            "diff3",
            "one\n<<<<<<< HEAD\nours\n||||||| BASE\ntwo\n=======\ntheirs\n>>>>>>> side\nthree\n",
        );
    }

    #[test]
    fn is_read_with_its_markers_in_the_zdiff3_style() {
        is_read_with_its_markers(
            "zdiff3",
            "one\n<<<<<<< HEAD\nours\n||||||| BASE\ntwo\n=======\ntheirs\n>>>>>>> side\nthree\n",
        );
    }

    #[test]
    fn is_marked_resolved_once_its_markers_are_gone_and_not_before() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("resolve", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("resolve").await.unwrap();
            let text = get_conflict(&repo, "f.txt").await.unwrap().text.unwrap();

            let error = mark_resolved(&repo, "f.txt", Some(&text.version), false)
                .await
                .unwrap_err();
            assert!(matches!(error, Error::ConflictMarkers { .. }), "{error:?}");
            let expected = SaveExpectation {
                version: text.version.clone(),
                overwrite: false,
            };
            let version =
                save_working_tree_file(&repo, "f.txt", "one\nours\ntheirs\nthree\n", &expected)
                    .await
                    .unwrap();
            // Resolved in the working tree, but not marked so yet.
            assert_eq!(status_marker_free(&repo).await, ["f.txt"]);
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("conflicted"), Some("conflicted"))])
            );

            mark_resolved(&repo, "f.txt", Some(&version), false)
                .await
                .unwrap();
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("modified"), None)])
            );
            assert_eq!(git(&path, ["show", ":f.txt"]), "one\nours\ntheirs\nthree");
            // Marking it again, once it isn't conflicted, does nothing.
            mark_resolved(&repo, "f.txt", Some(&version), false)
                .await
                .unwrap();
        });
    }

    #[test]
    fn is_marked_resolved_with_its_markers_when_the_user_says_they_belong_in_it() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("resolve-with-markers", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("resolve-with-markers").await.unwrap();
            let text = get_conflict(&repo, "f.txt").await.unwrap().text.unwrap();

            mark_resolved(&repo, "f.txt", Some(&text.version), true)
                .await
                .unwrap();
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("modified"), None)])
            );
            assert!(git(&path, ["show", ":f.txt"]).contains("<<<<<<< HEAD"));
        });
    }

    #[test]
    fn is_said_to_be_ready_and_no_longer_as_its_markers_go_and_come_back() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("ready-again", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("ready-again").await.unwrap();
            let markers = read(&path.join("f.txt"));
            assert_eq!(status_marker_free(&repo).await, Vec::<String>::new());

            fs::write(path.join("f.txt"), "one\nresolved\nthree\n").unwrap();
            assert_eq!(status_marker_free(&repo).await, ["f.txt"]);
            // Read again only once it changed: the same answer until then.
            assert_eq!(status_marker_free(&repo).await, ["f.txt"]);
            fs::write(path.join("f.txt"), markers).unwrap();
            assert_eq!(status_marker_free(&repo).await, Vec::<String>::new());
        });
    }

    #[test]
    fn isnt_marked_resolved_once_it_changed_on_disk_since_it_was_read() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("resolve-changed", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("resolve-changed").await.unwrap();
            let text = get_conflict(&repo, "f.txt").await.unwrap().text.unwrap();
            fs::write(path.join("f.txt"), "one\nchanged elsewhere\nthree\n").unwrap();

            let error = mark_resolved(&repo, "f.txt", Some(&text.version), false)
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::FileChangedOnDisk { .. }),
                "{error:?}"
            );
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("conflicted"), Some("conflicted"))])
            );
        });
    }

    #[test]
    fn isnt_staged_with_its_markers_whole_or_with_everything_else() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("refuse", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("refuse").await.unwrap();
            fs::write(path.join("README"), "changed\n").unwrap();

            let error = stage(&repo, &["f.txt", "README"]).await.unwrap_err();
            assert!(matches!(error, Error::ConflictMarkers { .. }), "{error:?}");
            assert_eq!(
                error.to_string(),
                "f.txt still has conflict markers. Resolve its conflicts, or mark it resolved with them from its changes if they belong in it."
            );
            let error = stage_all(&repo).await.unwrap_err();
            assert!(matches!(error, Error::ConflictMarkers { .. }), "{error:?}");
            // None was staged.
            assert_eq!(
                status_files(&repo).await,
                rows(&[
                    ("README", None, Some("modified")),
                    ("f.txt", Some("conflicted"), Some("conflicted")),
                ])
            );

            // Another file is, while the conflict is left.
            stage(&repo, &["README"]).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                rows(&[
                    ("README", Some("modified"), None),
                    ("f.txt", Some("conflicted"), Some("conflicted")),
                ])
            );
        });
    }

    #[test]
    fn is_staged_with_the_rest_once_its_markers_are_gone() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("stage-all", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("stage-all").await.unwrap();
            fs::write(path.join("f.txt"), "one\nresolved\nthree\n").unwrap();

            stage_all(&repo).await.unwrap();
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("modified"), None)])
            );
        });
    }

    #[test]
    fn keeps_a_side_whole() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("keep-theirs", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("keep-theirs").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();

            keep_side(&repo, "f.txt", Side::Theirs, &shown(&conflict))
                .await
                .unwrap();
            assert_eq!(read(&path.join("f.txt")), "one\ntheirs\nthree\n");
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("modified"), None)])
            );
        });
    }

    #[test]
    fn doesnt_keep_a_side_over_a_file_that_changed_on_disk_or_a_conflict_that_did() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("keep-changed", None, &text_conflict());
        smol::block_on(async {
            let repo = fx.repos.open("keep-changed").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();

            fs::write(path.join("f.txt"), "one\nedited\nthree\n").unwrap();
            let error = keep_side(&repo, "f.txt", Side::Ours, &shown(&conflict))
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::FileChangedOnDisk { .. }),
                "{error:?}"
            );
            git(&path, ["add", "f.txt"]);
            let unversioned = ShownConflict {
                sides: conflict.sides(),
                version: None,
            };
            let error = keep_side(&repo, "f.txt", Side::Ours, &unversioned)
                .await
                .unwrap_err();
            assert!(matches!(error, Error::ConflictChanged { .. }), "{error:?}");
            assert_eq!(read(&path.join("f.txt")), "one\nedited\nthree\n");
        });
    }

    // A file deleted on one side.

    fn deleted_by_them() -> Sides {
        Sides {
            base: files([("f.txt", "one\n")]),
            ours: files([("f.txt", "one\nours\n")]),
            theirs: Files::new().without("f.txt"),
        }
    }

    #[test]
    fn is_read_as_deleted_in_theirs() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("deleted-by-them", None, &deleted_by_them());
        smol::block_on(async {
            let repo = fx.repos.open("deleted-by-them").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            assert_eq!(conflict.base.unwrap().oid, oid(&path, ":1:f.txt"));
            assert_eq!(conflict.ours.unwrap().oid, oid(&path, ":2:f.txt"));
            assert_eq!(conflict.theirs, None);
            // Git leaves ours in the working tree, without markers.
            assert_eq!(conflict.text.unwrap().contents, "one\nours\n");
        });
    }

    #[test]
    fn is_deleted_by_keeping_theirs_or_kept_by_keeping_ours() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("delete-kept", None, &deleted_by_them());
        smol::block_on(async {
            let repo = fx.repos.open("delete-kept").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            keep_side(&repo, "f.txt", Side::Theirs, &shown(&conflict))
                .await
                .unwrap();
            assert!(!path.join("f.txt").exists());
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("deleted"), None)])
            );

            fx.create_merge_conflict("modify-kept", None, &deleted_by_them());
            let other = fx.repos.open("modify-kept").await.unwrap();
            let conflict = get_conflict(&other, "f.txt").await.unwrap();
            keep_side(&other, "f.txt", Side::Ours, &shown(&conflict))
                .await
                .unwrap();
            assert_eq!(status_files(&other).await, rows(&[]));
        });
    }

    #[test]
    fn is_read_as_deleted_in_ours_and_brought_back_by_keeping_theirs() {
        let fx = Fixture::new();
        let sides = Sides {
            base: files([("f.txt", "one\n")]),
            ours: Files::new().without("f.txt"),
            theirs: files([("f.txt", "one\ntheirs\n")]),
        };
        let path = fx.create_merge_conflict("deleted-by-us", None, &sides);
        smol::block_on(async {
            let repo = fx.repos.open("deleted-by-us").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            assert_eq!(conflict.ours, None);
            assert_eq!(
                conflict.theirs.as_ref().unwrap().oid,
                oid(&path, ":3:f.txt")
            );

            keep_side(&repo, "f.txt", Side::Theirs, &shown(&conflict))
                .await
                .unwrap();
            assert_eq!(read(&path.join("f.txt")), "one\ntheirs\n");
            assert_eq!(
                status_files(&repo).await,
                rows(&[("f.txt", Some("added"), None)])
            );
        });
    }

    // A file added on both sides.

    #[test]
    fn has_no_base_and_markers_for_the_whole_file() {
        let fx = Fixture::new();
        let sides = Sides {
            base: Files::new(),
            ours: files([("new.txt", "ours\n")]),
            theirs: files([("new.txt", "theirs\n")]),
        };
        fx.create_merge_conflict("added-by-both", None, &sides);
        smol::block_on(async {
            let repo = fx.repos.open("added-by-both").await.unwrap();
            let conflict = get_conflict(&repo, "new.txt").await.unwrap();
            assert_eq!(conflict.base, None);
            assert_eq!(conflict.ours.unwrap().mode, "100644");
            assert_eq!(conflict.theirs.unwrap().mode, "100644");
            assert_eq!(
                conflict.text.unwrap().contents,
                "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\n"
            );
        });
    }

    // A binary file.

    fn binary_sides() -> Sides {
        Sides {
            base: Files::new().with("b.bin", [0u8, 1, 2]),
            ours: Files::new().with("b.bin", [0u8, 1, 3]),
            theirs: Files::new().with("b.bin", [0u8, 1, 4]),
        }
    }

    #[test]
    fn isnt_read_as_text_and_has_no_markers() {
        let fx = Fixture::new();
        fx.create_merge_conflict("binary", None, &binary_sides());
        smol::block_on(async {
            let repo = fx.repos.open("binary").await.unwrap();
            let conflict = get_conflict(&repo, "b.bin").await.unwrap();
            assert_eq!(conflict.text, None);
            assert!(conflict.version.is_some());
            assert!(conflict.binary);
            assert_eq!(conflict.unreadable.as_deref(), Some("This file is binary."));
            // Nor is it ready to be marked resolved: git just left ours.
            assert_eq!(status_marker_free(&repo).await, Vec::<String>::new());
        });
    }

    #[test]
    fn is_resolved_by_keeping_a_side() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("binary-kept", None, &binary_sides());
        smol::block_on(async {
            let repo = fx.repos.open("binary-kept").await.unwrap();
            let conflict = get_conflict(&repo, "b.bin").await.unwrap();
            keep_side(&repo, "b.bin", Side::Theirs, &shown(&conflict))
                .await
                .unwrap();
            assert_eq!(fs::read(path.join("b.bin")).unwrap(), [0, 1, 4]);
            assert_eq!(
                status_files(&repo).await,
                rows(&[("b.bin", Some("modified"), None)])
            );
        });
    }

    // A file too large to show.

    #[test]
    fn is_still_checked_for_markers_and_for_changes_before_a_side_is_kept() {
        // Past `MAX_BLOB_BYTES`: its conflict at the top, a megabyte of lines after it.
        let tail = "line\n".repeat(1_300_000);
        let fx = Fixture::new();
        let sides = Sides {
            base: Files::new().with("f.txt", format!("two\n{tail}")),
            ours: Files::new().with("f.txt", format!("ours\n{tail}")),
            theirs: Files::new().with("f.txt", format!("theirs\n{tail}")),
        };
        let path = fx.create_merge_conflict("too-large", None, &sides);
        smol::block_on(async {
            let repo = fx.repos.open("too-large").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            assert_eq!(conflict.text, None);
            assert!(
                conflict.version.as_ref().unwrap().starts_with("stamp:"),
                "{:?}",
                conflict.version
            );
            assert_eq!(
                conflict.unreadable.as_deref(),
                Some("This file is too large to show.")
            );
            let error = stage(&repo, &["f.txt"]).await.unwrap_err();
            assert!(matches!(error, Error::ConflictMarkers { .. }), "{error:?}");

            fs::write(path.join("f.txt"), format!("edited\n{tail}")).unwrap();
            let error = keep_side(&repo, "f.txt", Side::Ours, &shown(&conflict))
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::FileChangedOnDisk { .. }),
                "{error:?}"
            );
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            keep_side(&repo, "f.txt", Side::Ours, &shown(&conflict))
                .await
                .unwrap();
            assert_eq!(status_files(&repo).await, rows(&[]));
        });
    }

    // A symbolic link.

    #[cfg(unix)]
    #[test]
    fn isnt_followed_even_out_of_the_repository_and_is_resolved_by_keeping_a_side() {
        use std::os::unix::fs::symlink;

        let fx = Fixture::new();
        let path = fx.create_repo("link-conflict");
        git(&path, ["config", "core.symlinks", "true"]);
        fs::write(path.join("README"), "readme\n").unwrap();
        symlink("/etc/hosts", path.join("config")).unwrap();
        git(&path, ["add", "-A"]);
        git(&path, ["commit", "-qm", "base"]);
        git(&path, ["checkout", "-qb", "side"]);
        fs::remove_file(path.join("config")).unwrap();
        symlink("../theirs", path.join("config")).unwrap();
        git(&path, ["commit", "-qam", "theirs"]);
        git(&path, ["checkout", "-q", "main"]);
        fs::remove_file(path.join("config")).unwrap();
        symlink("/etc/ours", path.join("config")).unwrap();
        git(&path, ["commit", "-qam", "ours"]);
        try_git(&path, ["merge", "-q", "side"]);
        smol::block_on(async {
            let repo = fx.repos.open("link-conflict").await.unwrap();

            let conflict = get_conflict(&repo, "config").await.unwrap();
            assert_eq!(conflict.ours.as_ref().unwrap().mode, "120000");
            assert_eq!(conflict.theirs.as_ref().unwrap().mode, "120000");
            assert_eq!(conflict.text, None);
            assert_eq!(conflict.version, None);
            assert_eq!(conflict.unreadable, None);
            keep_side(&repo, "config", Side::Theirs, &shown(&conflict))
                .await
                .unwrap();
            assert_eq!(
                fs::read_link(path.join("config")).unwrap(),
                Path::new("../theirs")
            );
            assert_eq!(
                status_files(&repo).await,
                rows(&[("config", Some("modified"), None)])
            );
        });
    }

    // A file that isn't conflicted.

    #[test]
    fn has_no_sides() {
        let fx = Fixture::new();
        let path = fx.create_merge_conflict("not-conflicted", None, &text_conflict());
        try_git(&path, ["merge", "--abort"]);
        smol::block_on(async {
            let repo = fx.repos.open("not-conflicted").await.unwrap();
            let conflict = get_conflict(&repo, "f.txt").await.unwrap();
            assert_eq!(conflict.base, None);
            assert_eq!(conflict.ours, None);
            assert_eq!(conflict.theirs, None);
            assert_eq!(conflict.text.unwrap().contents, "one\nours\nthree\n");
        });
    }
}
