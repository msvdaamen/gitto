//! Creating and switching branches, taking the uncommitted changes along. Ported from
//! `commands.ts`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::core::errors::{Error, GitError, Result};
use crate::core::repo::{GitCommand, Repo, current_branch, resolve_ref, succeeds};
use crate::core::runner::RunOptions;
use crate::features::refs::parse::{RefName, parse_ref_name};
use crate::features::refs::schema::RefKind;
use crate::features::remote::commands::list_remotes;
use crate::features::remote::remote_name::remote_of;
use crate::features::stash::commands::{pop_newest, stash_changes};

/// Whether `value` has something but whitespace, as JavaScript's `/\S/` has it: Unicode's
/// whitespace, and the byte order mark.
pub(crate) fn has_non_space(value: &str) -> bool {
    value.chars().any(|c| !c.is_whitespace() && c != '\u{feff}')
}

/// Without the hint pointing to `git help check-ref-format` after an invalid name.
pub(crate) fn no_ref_syntax_advice() -> RunOptions {
    RunOptions::default().config("advice.refSyntax=false")
}

/// `ref`'s kind and short name, if it's a branch, local or remote; `None` for a tag or anything
/// else.
pub(crate) fn branch_name(reference: &str) -> Option<RefName> {
    parse_ref_name(reference).filter(|target| target.kind != RefKind::Tag)
}

/// Creates the branch `name` and switches to it: at HEAD, or at the branch `from` (a full ref name),
/// which must be there.
///
/// At HEAD, it stays on the same commit, so every uncommitted change, staged or not, comes along to
/// the new branch as it is; from a branch, they come along as when switching to it (see
/// `switch_branch`), through the stash if need be. It doesn't track the branch it was made from, so
/// pulling it doesn't pull that one's commits into it. Git refuses a name that's taken or not a
/// valid branch name, and to switch in the middle of a merge or rebase, saying so.
///
/// Fails with `Error::InvalidInput` if `name` is only whitespace, as the UI's input is checked.
pub async fn create_branch(repo: &Repo, name: &str, from: Option<&str>) -> Result<()> {
    if !has_non_space(name) {
        return Err(Error::InvalidInput("The name is empty.".to_owned()));
    }
    let args: Vec<String> = ["switch", "--no-track", "--create", name]
        .map(str::to_owned)
        .to_vec();
    let Some(from) = from else {
        repo.write_with(args, no_ref_syntax_advice()).await?;
        return Ok(());
    };
    if branch_name(from).is_none() {
        return Err(Error::Bug(format!("{from} isn't a branch.")));
    }
    let root = repo.path().to_owned();
    // One write, so nothing else changes the branches, the stashes or the working tree halfway.
    repo.exclusive(|run| async move {
        // That ref itself, rather than git reading `from` as a revision, like `refs/heads/main~3`.
        if !is_ref(&run, from).await? {
            return Err(Error::Bug(format!("{from} isn't a branch.")));
        }
        let mut args = args;
        args.extend(["--".to_owned(), from.to_owned()]);
        switch_taking_changes(&run, &root, args, name, no_ref_syntax_advice()).await
    })
    .await
}

/// Whether `ref` is the full name of a ref that's there.
pub async fn is_ref(run: &GitCommand, reference: &str) -> Result<bool> {
    // Exits with 1, saying nothing, when there's no such ref.
    succeeds(run, ["show-ref", "--verify", "--quiet", reference]).await
}

/// What git says when it won't switch as it'd lose uncommitted changes or untracked files: ones the
/// branch would overwrite or remove, or that are in a folder it replaces with a file.
static WOULD_LOSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"would be (overwritten|removed) by checkout|would lose untracked files")
        .expect("a valid regex")
});

/// Switches to the branch `ref` (a full ref name), taking the uncommitted changes along, staged or
/// not, untracked files included. For a remote branch, that's the local branch tracking it (the
/// current one, or the one named after it, if several do), or a new one that does, named after it
/// without the remote (e.g. `feature` for `origin/feature`); git refuses if a local branch that
/// doesn't track it has that name.
///
/// When git won't switch with the changes, as the branch changes the files they're in, they're
/// stashed, the branch switched to, and the stash popped there; if it wouldn't pop without
/// conflicts, it's kept, and that's returned as an `Error::ChangesStashed`. Git refuses to switch in
/// the middle of a merge or rebase, saying so.
pub async fn switch_branch(repo: &Repo, reference: &str) -> Result<()> {
    let Some(target) = branch_name(reference) else {
        return Err(Error::Bug(format!("{reference} isn't a branch.")));
    };
    let root = repo.path().to_owned();
    // One write, so nothing else changes the branches, the stashes or the working tree halfway.
    repo.exclusive(|run| async move {
        let args = switch_args(&run, reference, &target).await?;
        switch_taking_changes(&run, &root, args, &target.name, RunOptions::default()).await
    })
    .await
}

/// Runs the `git switch` `args`, to the branch `name`, taking the uncommitted changes along: when
/// git won't, as the branch changes the files they're in, they're stashed, it's switched, and the
/// stash popped there; if it wouldn't pop without conflicts, it's kept, and that's returned as an
/// `Error::ChangesStashed`. For a repository at `root`, in a write of its own (`Repo::exclusive`).
async fn switch_taking_changes(
    run: &GitCommand,
    root: &Path,
    args: Vec<String>,
    name: &str,
    options: RunOptions,
) -> Result<()> {
    // It rewrites the files that differ between the branches.
    let switching = options.rewrites_files();
    // Git takes the changes along itself, as long as the branch doesn't change their files.
    let refused = match run.run_with(&args, switching.clone()).await {
        Ok(_) => return Ok(()),
        Err(Error::Git(error)) if WOULD_LOSE.is_match(&error.stderr) => error,
        Err(error) => return Err(error),
    };

    let stash = stash_to_switch(run, name, refused).await?;
    let from = head_position(run).await?;
    let mut failed = None;
    if let Err(error) = run.run_with(&args, switching).await {
        // Back as they were, on the branch they were made on, when it didn't switch.
        if head_position(run).await? == from {
            pop_newest(run).await?;
            return Err(error);
        }
        // It switched, but failed after, at a post-checkout hook, say: returned once they're back.
        failed = Some(error);
    }
    if !pops_cleanly(run, root, &stash).await? {
        let branch = current_branch(run).await?;
        return Err(Error::ChangesStashed(format!(
            "Switched to {}, but your uncommitted changes conflict with it, so they're kept in the stash.",
            branch.as_deref().unwrap_or("the branch")
        )));
    }
    pop_newest(run).await?;
    match failed {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The `git switch` that switches to the branch `ref`, of `target`'s kind and short name.
async fn switch_args(run: &GitCommand, reference: &str, target: &RefName) -> Result<Vec<String>> {
    let args =
        |args: &[&str]| -> Vec<String> { args.iter().map(|arg| (*arg).to_owned()).collect() };
    // `--no-guess`: never a new branch made from a remote one of the same name.
    if target.kind != RefKind::Remote {
        return Ok(args(&["switch", "--no-guess", "--", &target.name]));
    }
    Ok(match tracking_branch(run, reference, &target.name).await? {
        Tracking::Existing(existing) => args(&["switch", "--no-guess", "--", &existing]),
        Tracking::Local(local) => args(&[
            "switch",
            &format!("--create={local}"),
            "--track",
            "--",
            reference,
        ]),
    })
}

/// The local branch for a remote branch (see `tracking_branch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tracking {
    /// The local branch tracking it, by its short name.
    Existing(String),
    /// None does: the name for a new one that would.
    Local(String),
}

/// The local branch for the remote branch `ref` (its full name), named `name` (e.g.
/// `origin/feature`): the one tracking it (the current one, or the one named after it, if several
/// do), as `Existing`; or, when none does, the name for a new one that would, as `Local`: the
/// branch's name without the remote (`feature`).
pub async fn tracking_branch(run: &GitCommand, reference: &str, name: &str) -> Result<Tracking> {
    let (tracking, remotes) =
        futures::try_join!(tracking_locals(run, reference), list_remotes(run))?;
    // Without the remote, or (the remote's gone, its branches left) the name's first part.
    let local = match remote_of(&remotes, name) {
        Some(remote) => &name[remote.len() + 1..],
        None => name.find('/').map_or(name, |slash| &name[slash + 1..]),
    };
    let branch = tracking
        .iter()
        .find(|candidate| candidate.current)
        .or_else(|| tracking.iter().find(|candidate| candidate.name == local))
        .or_else(|| tracking.first());
    Ok(match branch {
        Some(branch) => Tracking::Existing(branch.name.clone()),
        None => Tracking::Local(local.to_owned()),
    })
}

/// A local branch tracking a remote one (see `tracking_locals`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackingLocal {
    /// Its short name.
    pub name: String,
    /// Whether it's the one checked out here.
    pub current: bool,
}

/// The local branches tracking the remote branch `ref` (its full name), by their short names; the
/// one checked out here is `current`.
pub async fn tracking_locals(run: &GitCommand, reference: &str) -> Result<Vec<TrackingLocal>> {
    // With NUL-separated fields: ref names can't contain one.
    let locals = run
        .run([
            "for-each-ref",
            "--format=%(refname:lstrip=2)%00%(upstream)%00%(HEAD)",
            "refs/heads/",
        ])
        .await?;
    Ok(locals
        .split('\n')
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let (name, upstream, head) = (fields.next()?, fields.next()?, fields.next());
            (upstream == reference).then(|| TrackingLocal {
                name: name.to_owned(),
                current: head == Some("*"),
            })
        })
        .collect())
}

/// Stashes the uncommitted changes git `refused` to switch to `name` with, and returns the stash.
/// When nothing could be stashed (before the first commit, say), `refused` is returned: why git
/// wouldn't switch says more than why it wouldn't stash.
async fn stash_to_switch(run: &GitCommand, name: &str, refused: GitError) -> Result<String> {
    let before = resolve_ref(run, "refs/stash").await?;
    let message = format!("Uncommitted changes when switching to {name}");
    let failure = stash_changes(run, Some(&message)).await.err();
    let stash = resolve_ref(run, "refs/stash").await?;
    let Some(stash) = stash.filter(|stash| Some(stash) != before.as_ref()) else {
        return Err(Error::Git(refused));
    };
    // Git can fail after making the stash, e.g. removing an untracked file it stashed.
    if let Some(failure) = failure {
        return Err(Error::ChangesStashed(format!(
            "Stashing your uncommitted changes failed partway, so they're in the stash, and some may still be here too. {failure}"
        )));
    }
    Ok(stash)
}

/// The branch HEAD is on, or the commit, when it's detached.
async fn head_position(run: &GitCommand) -> Result<Option<String>> {
    match current_branch(run).await? {
        Some(branch) => Ok(Some(branch)),
        None => resolve_ref(run, "HEAD").await,
    }
}

/// Whether the stash `sha` would pop without conflicts, onto a working tree without changes: its
/// changes merge with HEAD's, and none of its untracked files is in the way of a file that's there.
/// Checked up front, as a pop that fails can leave some of the changes applied.
async fn pops_cleanly(run: &GitCommand, root: &Path, sha: &str) -> Result<bool> {
    // The merge a pop makes, without touching the working tree; exits with 1 at conflicts.
    let merge_base = format!("--merge-base={sha}^1");
    let merges = succeeds(
        run,
        ["merge-tree", "--write-tree", &merge_base, "HEAD", sha],
    )
    .await?;
    if !merges {
        return Ok(false);
    }

    // The untracked files are in a third parent, if it stashed any.
    let untracked_commit = format!("{sha}^3");
    if resolve_ref(run, &untracked_commit).await?.is_none() {
        return Ok(true);
    }
    // As bytes, so a name that isn't UTF-8 is looked for as it is.
    let output = run
        .run_bytes(["ls-tree", "-r", "-z", "--name-only", &untracked_commit])
        .await?;
    let untracked: Vec<PathBuf> = output
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .map(|name| root.join(path_of(name)))
        .collect();
    // In batches, stopping at the first that's in the way: there can be thousands.
    for batch in untracked.chunks(100) {
        let found = futures::future::join_all(batch.iter().map(|file| exists(file))).await;
        if found.contains(&true) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The path git names with `bytes`.
fn path_of(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// Whether something is at `path`, or a file is where one of its folders would go.
async fn exists(path: &Path) -> bool {
    match async_fs::symlink_metadata(path).await {
        Ok(_) => true,
        // ENOTDIR when one of its folders is a file; anything else may be in the way too.
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::test_support::{Fixture, git, wait_for, write_script};

    /// A repository with `a.txt` committed on `main`, which `repos` opens as `name`.
    fn create_committed_repo(fx: &Fixture, name: &str) -> PathBuf {
        let path = fx.create_repo(name);
        fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "first"]);
        path
    }

    /// A committed repository `name` with an `other` branch that changes `a.txt` and adds
    /// `other.txt`, on `main`.
    fn create_branched_repo(fx: &Fixture, name: &str) -> PathBuf {
        let path = create_committed_repo(fx, name);
        fs::write(path.join("b.txt"), "b\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "second"]);
        git(&path, ["switch", "-qc", "other"]);
        fs::write(path.join("a.txt"), "other\na\n").unwrap();
        fs::write(path.join("other.txt"), "other\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "other"]);
        git(&path, ["switch", "-q", "main"]);
        path
    }

    fn read(path: &Path, file: &str) -> String {
        fs::read_to_string(path.join(file)).unwrap()
    }

    fn message(error: &Error) -> String {
        error.to_string()
    }

    // Creating a branch.

    #[test]
    fn switches_to_a_new_branch_at_head_taking_the_uncommitted_changes_along() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "changes");
        fs::write(path.join("a.txt"), "staged\n").unwrap();
        git(&path, ["add", "a.txt"]);
        fs::write(path.join("a.txt"), "unstaged\n").unwrap();
        fs::write(path.join("new.txt"), "new\n").unwrap();
        let before = git(&path, ["status", "--porcelain"]);

        smol::block_on(async {
            let repo = fx.repos.open("changes").await.unwrap();
            create_branch(&repo, "feature/new", None).await.unwrap();
        });
        assert_eq!(
            git(&path, ["symbolic-ref", "--short", "HEAD"]),
            "feature/new"
        );
        assert_eq!(
            git(&path, ["rev-parse", "feature/new"]),
            git(&path, ["rev-parse", "main"])
        );
        assert_eq!(git(&path, ["status", "--porcelain"]), before);
    }

    #[test]
    fn doesnt_track_the_branch_it_was_made_from() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "upstream");
        let clone = fx.clone_repo("upstream-clone", &path);
        // Else a branch made from `main` would track `origin/main` too.
        git(&clone, ["config", "branch.autoSetupMerge", "inherit"]);

        smol::block_on(async {
            let repo = fx.repos.open("upstream-clone").await.unwrap();
            create_branch(&repo, "topic", None).await.unwrap();
        });
        assert_eq!(
            git(
                &clone,
                ["for-each-ref", "--format=%(upstream)", "refs/heads/topic"]
            ),
            ""
        );
    }

    #[test]
    fn works_before_the_first_commit() {
        let fx = Fixture::new();
        let path = fx.create_repo("unborn");
        fs::write(path.join("a.txt"), "a\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("unborn").await.unwrap();
            create_branch(&repo, "other", None).await.unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(git(&path, ["status", "--porcelain"]), "?? a.txt");
    }

    #[test]
    fn says_why_a_name_cant_be_used() {
        let fx = Fixture::new();
        create_committed_repo(&fx, "names");
        smol::block_on(async {
            let repo = fx.repos.open("names").await.unwrap();

            let error = create_branch(&repo, "main", None).await.unwrap_err();
            assert!(
                message(&error).contains("fatal: a branch named 'main' already exists"),
                "{error}"
            );
            let error = create_branch(&repo, "a..b", None).await.unwrap_err();
            assert_eq!(message(&error), "fatal: 'a..b' is not a valid branch name");
            let error = create_branch(&repo, "--force", None).await.unwrap_err();
            assert!(
                message(&error).contains("not a valid branch name"),
                "{error}"
            );
        });
    }

    #[test]
    fn refuses_a_name_thats_only_whitespace() {
        let fx = Fixture::new();
        create_committed_repo(&fx, "blank");
        smol::block_on(async {
            let repo = fx.repos.open("blank").await.unwrap();
            let error = create_branch(&repo, " \t", None).await.unwrap_err();
            assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
            assert_eq!(message(&error), "The name is empty.");
        });
    }

    // Switching branches.

    #[test]
    fn takes_the_changes_along_when_theyre_in_files_the_branch_doesnt_change() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-along");
        fs::write(path.join("b.txt"), "staged\n").unwrap();
        git(&path, ["add", "b.txt"]);
        fs::write(path.join("new.txt"), "new\n").unwrap();
        let before = git(&path, ["status", "--porcelain"]);

        smol::block_on(async {
            let repo = fx.repos.open("switch-along").await.unwrap();
            switch_branch(&repo, "refs/heads/other").await.unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(git(&path, ["status", "--porcelain"]), before);
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn refreshes_the_index_a_second_later_so_git_doesnt_read_every_file_it_wrote_again() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-refresh");
        smol::block_on(async {
            let repo = fx.repos.open("switch-refresh").await.unwrap();
            switch_branch(&repo, "refs/heads/other").await.unwrap();

            // Git compares them in whole seconds: until the index is written in a later second than
            // the files, it can't tell from it whether they changed since.
            let second = |file: &str| {
                fs::metadata(path.join(file))
                    .unwrap()
                    .modified()
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            };
            wait_for(Duration::from_secs(3), || {
                second(".git/index") > second("other.txt")
            })
            .await;
        });
    }

    #[test]
    fn brings_changes_to_files_the_branch_changes_along_when_they_merge() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-merge");
        // `other` changes the first line; this the last.
        fs::write(path.join("a.txt"), "a\nmine\n").unwrap();
        fs::write(path.join("new.txt"), "new\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-merge").await.unwrap();
            switch_branch(&repo, "refs/heads/other").await.unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(read(&path, "a.txt"), "other\na\nmine\n");
        assert_eq!(git(&path, ["status", "--porcelain"]), "M a.txt\n?? new.txt");
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn keeps_the_changes_in_the_stash_when_they_conflict_with_the_branch() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-conflict");
        fs::write(path.join("a.txt"), "mine\n").unwrap();
        fs::write(path.join("new.txt"), "new\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-conflict").await.unwrap();
            let error = switch_branch(&repo, "refs/heads/other").await.unwrap_err();
            assert!(
                message(&error).contains(
                    "Switched to other, but your uncommitted changes conflict with it, so they're kept in the stash."
                ),
                "{error}"
            );
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(git(&path, ["status", "--porcelain"]), "");
        assert_eq!(
            git(&path, ["stash", "list"]),
            "stash@{0}: On main: Uncommitted changes when switching to other"
        );
        git(&path, ["switch", "-q", "main"]);
        git(&path, ["stash", "pop", "-q"]);
        assert_eq!(read(&path, "a.txt"), "mine\n");
        assert_eq!(read(&path, "new.txt"), "new\n");
    }

    #[test]
    fn keeps_the_changes_in_the_stash_when_an_untracked_file_is_in_the_way() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-untracked");
        // Would merge, but `other` has an `other.txt` of its own.
        fs::write(path.join("b.txt"), "mine\n").unwrap();
        fs::write(path.join("other.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-untracked").await.unwrap();
            let error = switch_branch(&repo, "refs/heads/other").await.unwrap_err();
            assert!(message(&error).contains("kept in the stash"), "{error}");
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(git(&path, ["status", "--porcelain"]), "");
        assert_eq!(read(&path, "other.txt"), "other\n");
        assert!(
            git(&path, ["stash", "list"]).contains("Uncommitted changes when switching to other")
        );
    }

    #[test]
    fn leaves_the_changes_and_the_stashes_be_when_git_wont_switch() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-refused");
        fs::write(path.join("a.txt"), "mine\n").unwrap();
        smol::block_on(async {
            let repo = fx.repos.open("switch-refused").await.unwrap();

            let error = switch_branch(&repo, "refs/heads/missing")
                .await
                .unwrap_err();
            assert!(
                message(&error).contains("invalid reference: missing"),
                "{error}"
            );
            let error = switch_branch(&repo, "refs/heads/--force")
                .await
                .unwrap_err();
            assert!(message(&error).contains("invalid reference"), "{error}");
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(git(&path, ["status", "--porcelain"]), "M a.txt");
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn switches_to_the_local_branch_tracking_a_remote_one() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "switch-tracked-origin");
        let path = fx.clone_repo("switch-tracked", &origin);
        git(&path, ["switch", "-qc", "mine", "--track", "origin/other"]);
        git(&path, ["switch", "-q", "main"]);
        fs::write(path.join("b.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-tracked").await.unwrap();
            switch_branch(&repo, "refs/remotes/origin/other")
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "mine");
        assert_eq!(
            git(&path, ["branch", "--format=%(refname:short)"]),
            "main\nmine"
        );
        assert_eq!(git(&path, ["status", "--porcelain"]), "M b.txt");
    }

    #[test]
    fn makes_a_local_branch_tracking_a_remote_one_when_none_does() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "switch-remote-origin");
        let path = fx.clone_repo("switch-remote", &origin);
        fs::write(path.join("a.txt"), "a\nmine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-remote").await.unwrap();
            switch_branch(&repo, "refs/remotes/origin/other")
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(
            git(&path, ["rev-parse", "--abbrev-ref", "other@{upstream}"]),
            "origin/other"
        );
        // Brought along through the stash, as `other` changes `a.txt`.
        assert_eq!(read(&path, "a.txt"), "other\na\nmine\n");
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn names_the_branch_it_made_when_the_changes_are_kept_in_the_stash() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "switch-remote-conflict-origin");
        let path = fx.clone_repo("switch-remote-conflict", &origin);
        fs::write(path.join("a.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-remote-conflict").await.unwrap();
            let error = switch_branch(&repo, "refs/remotes/origin/other")
                .await
                .unwrap_err();
            assert!(
                message(&error).contains(
                    "Switched to other, but your uncommitted changes conflict with it, so they're kept in the stash."
                ),
                "{error}"
            );
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(
            git(&path, ["stash", "list"]),
            "stash@{0}: On main: Uncommitted changes when switching to origin/other"
        );
    }

    #[test]
    fn stashes_untracked_files_in_a_folder_the_branch_replaces_with_a_file() {
        let fx = Fixture::new();
        let path = create_committed_repo(&fx, "switch-folder");
        git(&path, ["switch", "-qc", "file"]);
        fs::write(path.join("dir"), "file\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "file"]);
        git(&path, ["switch", "-q", "main"]);
        fs::create_dir(path.join("dir")).unwrap();
        fs::write(path.join("dir").join("tracked.txt"), "tracked\n").unwrap();
        git(&path, ["add", "."]);
        git(&path, ["commit", "-qm", "folder"]);
        git(&path, ["switch", "-q", "file"]);
        git(&path, ["switch", "-q", "main"]);
        fs::write(path.join("dir").join("untracked.txt"), "mine\n").unwrap();

        // Git won't switch, as that'd lose `dir/untracked.txt`; it's stashed, and kept, as `dir` is
        // a file there.
        smol::block_on(async {
            let repo = fx.repos.open("switch-folder").await.unwrap();
            let error = switch_branch(&repo, "refs/heads/file").await.unwrap_err();
            assert!(message(&error).contains("kept in the stash"), "{error}");
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "file");
        assert!(
            git(&path, ["stash", "list"]).contains("Uncommitted changes when switching to file")
        );
    }

    #[test]
    fn names_the_branch_it_switched_to_when_a_tag_has_its_name_too() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-tag");
        git(&path, ["tag", "other"]);
        fs::write(path.join("a.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-tag").await.unwrap();
            let error = switch_branch(&repo, "refs/heads/other").await.unwrap_err();
            assert!(
                message(&error).starts_with("Switched to other, but"),
                "{error}"
            );
        });
    }

    #[test]
    fn brings_the_changes_along_when_it_switched_but_a_hook_failed_after() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "switch-hook");
        write_script(
            &path.join(".git").join("hooks").join("post-checkout"),
            "#!/bin/sh\necho hook failed >&2\nexit 1\n",
        );
        fs::write(path.join("a.txt"), "a\nmine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("switch-hook").await.unwrap();
            let error = switch_branch(&repo, "refs/heads/other").await.unwrap_err();
            assert!(message(&error).contains("hook failed"), "{error}");
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(read(&path, "a.txt"), "other\na\nmine\n");
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn names_a_branch_made_from_a_remote_one_after_it_without_the_remote_slashes_and_all() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "switch-slash-origin");
        let path = fx.create_repo("switch-slash");
        git(
            &path,
            ["remote", "add", "up/stream", origin.to_str().unwrap()],
        );
        git(&path, ["fetch", "-q", "up/stream"]);

        smol::block_on(async {
            let repo = fx.repos.open("switch-slash").await.unwrap();
            switch_branch(&repo, "refs/remotes/up/stream/other")
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
        assert_eq!(
            git(&path, ["rev-parse", "--abbrev-ref", "other@{upstream}"]),
            "up/stream/other"
        );
    }

    #[test]
    fn picks_the_current_branch_then_the_one_named_after_it_when_several_track_a_remote_one() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "switch-several-origin");
        let path = fx.clone_repo("switch-several", &origin);
        git(&path, ["branch", "-q", "--track", "a-mine", "origin/other"]);
        git(&path, ["branch", "-q", "--track", "other", "origin/other"]);
        smol::block_on(async {
            let repo = fx.repos.open("switch-several").await.unwrap();

            switch_branch(&repo, "refs/remotes/origin/other")
                .await
                .unwrap();
            assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "other");
            git(&path, ["switch", "-q", "a-mine"]);
            switch_branch(&repo, "refs/remotes/origin/other")
                .await
                .unwrap();
            assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "a-mine");
        });
    }

    // Creating a branch from another.

    #[test]
    fn switches_to_a_new_branch_at_it_taking_the_changes_along() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "create-from");
        fs::write(path.join("b.txt"), "staged\n").unwrap();
        git(&path, ["add", "b.txt"]);
        fs::write(path.join("new.txt"), "new\n").unwrap();
        let before = git(&path, ["status", "--porcelain"]);

        smol::block_on(async {
            let repo = fx.repos.open("create-from").await.unwrap();
            create_branch(&repo, "topic", Some("refs/heads/other"))
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "topic");
        assert_eq!(
            git(&path, ["rev-parse", "topic"]),
            git(&path, ["rev-parse", "other"])
        );
        assert_eq!(git(&path, ["status", "--porcelain"]), before);
    }

    #[test]
    fn brings_changes_to_files_it_changes_along_when_they_merge() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "create-from-merge");
        fs::write(path.join("a.txt"), "a\nmine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("create-from-merge").await.unwrap();
            create_branch(&repo, "topic", Some("refs/heads/other"))
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "topic");
        assert_eq!(read(&path, "a.txt"), "other\na\nmine\n");
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn keeps_the_changes_in_the_stash_when_they_conflict_with_it() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "create-from-conflict");
        fs::write(path.join("a.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("create-from-conflict").await.unwrap();
            let error = create_branch(&repo, "topic", Some("refs/heads/other"))
                .await
                .unwrap_err();
            assert!(
                message(&error).contains(
                    "Switched to topic, but your uncommitted changes conflict with it, so they're kept in the stash."
                ),
                "{error}"
            );
        });
        assert_eq!(
            git(&path, ["rev-parse", "topic"]),
            git(&path, ["rev-parse", "other"])
        );
        assert_eq!(git(&path, ["status", "--porcelain"]), "");
        assert_eq!(
            git(&path, ["stash", "list"]),
            "stash@{0}: On main: Uncommitted changes when switching to topic"
        );
    }

    #[test]
    fn makes_one_from_a_remote_branch_that_doesnt_track_it() {
        let fx = Fixture::new();
        let origin = create_branched_repo(&fx, "create-from-remote-origin");
        let path = fx.clone_repo("create-from-remote", &origin);
        git(&path, ["config", "branch.autoSetupMerge", "always"]);

        smol::block_on(async {
            let repo = fx.repos.open("create-from-remote").await.unwrap();
            create_branch(&repo, "topic", Some("refs/remotes/origin/other"))
                .await
                .unwrap();
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "topic");
        assert_eq!(
            git(&path, ["rev-parse", "topic"]),
            git(&path, ["rev-parse", "origin/other"])
        );
        assert_eq!(
            git(
                &path,
                ["for-each-ref", "--format=%(upstream)", "refs/heads/topic"]
            ),
            ""
        );
    }

    #[test]
    fn leaves_everything_be_when_the_name_is_taken() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "create-from-taken");
        fs::write(path.join("a.txt"), "mine\n").unwrap();

        smol::block_on(async {
            let repo = fx.repos.open("create-from-taken").await.unwrap();
            let error = create_branch(&repo, "main", Some("refs/heads/other"))
                .await
                .unwrap_err();
            assert!(
                message(&error).contains("fatal: a branch named 'main' already exists"),
                "{error}"
            );
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(git(&path, ["status", "--porcelain"]), "M a.txt");
        assert_eq!(git(&path, ["stash", "list"]), "");
    }

    #[test]
    fn refuses_to_make_one_from_what_isnt_a_branch() {
        let fx = Fixture::new();
        let path = create_branched_repo(&fx, "create-from-other-refs");
        git(&path, ["tag", "v1"]);
        smol::block_on(async {
            let repo = fx.repos.open("create-from-other-refs").await.unwrap();

            for from in ["refs/heads/other~1", "refs/heads/missing", "refs/tags/v1"] {
                let error = create_branch(&repo, "topic", Some(from)).await.unwrap_err();
                assert!(
                    message(&error).contains(&format!("{from} isn't a branch.")),
                    "{error}"
                );
            }
        });
        assert_eq!(git(&path, ["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(
            git(&path, ["branch", "--format=%(refname:short)"]),
            "main\nother"
        );
    }
}
