//! Errors the git crate returns. They describe what went wrong in git terms and carry a message a
//! user can read, and each says how the UI should take it (`api`). An error whose `api` is
//! `ApiCode::InternalServerError`, or `ApiCode::BadRequest` (input the UI shouldn't have sent), is
//! a bug, reported as such (see `ApiCode::is_bug`).
//!
//! Ported from `packages/git/src/core/errors.ts`, where each error is a class; here each is a
//! variant of [`Error`], and the failures of git commands share [`GitError`].

use std::fmt;
use std::io;
use std::sync::Arc;

/// What the git crate's functions return.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// How an error reaches the UI: what kind of failure it is. In the Electron app it reached the
/// renderer as an oRPC error with `code()` and `status()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiCode {
    /// No such repository, folder or file.
    NotFound,
    /// What was asked can't be done as things are; nothing was done.
    PreconditionFailed,
    /// The repository changed under what was asked, or what it left is for the user to finish.
    Conflict,
    PayloadTooLarge,
    Forbidden,
    UnsupportedMediaType,
    UnprocessableContent,
    /// Input that isn't valid, e.g. a SHA that isn't hex (see `crate::input`).
    BadRequest,
    /// Git failing in a way Gitto doesn't expect: a bug to report.
    InternalServerError,
    /// A push turned down that force-pushing would get past (see [`Error::PushRejected`]).
    PushRewritten,
    /// A push turned down as someone else pushed first (see [`Error::PushRejected`]).
    PushBehind,
}

impl ApiCode {
    /// The code as the renderer knew it, e.g. `NOT_FOUND`.
    pub fn code(self) -> &'static str {
        match self {
            ApiCode::NotFound => "NOT_FOUND",
            ApiCode::PreconditionFailed => "PRECONDITION_FAILED",
            ApiCode::Conflict => "CONFLICT",
            ApiCode::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            ApiCode::Forbidden => "FORBIDDEN",
            ApiCode::UnsupportedMediaType => "UNSUPPORTED_MEDIA_TYPE",
            ApiCode::UnprocessableContent => "UNPROCESSABLE_CONTENT",
            ApiCode::BadRequest => "BAD_REQUEST",
            ApiCode::InternalServerError => "INTERNAL_SERVER_ERROR",
            ApiCode::PushRewritten => "PUSH_REWRITTEN",
            ApiCode::PushBehind => "PUSH_BEHIND",
        }
    }

    /// The HTTP status the error had in the Electron app: oRPC's for its codes, and 409 for
    /// Gitto's own, which `errors.ts` gave one as oRPC has none for them.
    pub fn status(self) -> u16 {
        match self {
            ApiCode::BadRequest => 400,
            ApiCode::Forbidden => 403,
            ApiCode::NotFound => 404,
            ApiCode::Conflict | ApiCode::PushRewritten | ApiCode::PushBehind => 409,
            ApiCode::PreconditionFailed => 412,
            ApiCode::PayloadTooLarge => 413,
            ApiCode::UnsupportedMediaType => 415,
            ApiCode::UnprocessableContent => 422,
            ApiCode::InternalServerError => 500,
        }
    }

    /// Whether this is a bug to report rather than something to tell the user (`isBug` in
    /// `packages/rpc/src/server.ts`): git failing in a way Gitto doesn't expect, or input that the
    /// UI shouldn't have sent, which is the UI's bug.
    pub fn is_bug(self) -> bool {
        self.status() >= 500 || self == ApiCode::BadRequest
    }
}

/// Which kind of failure a [`GitError`] is: TS had a subclass of `GitError` for each but `Command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitErrorKind {
    /// Any other failure, or git that couldn't start.
    Command,
    /// The repository's folder is no longer a git repository, e.g. its `.git` was deleted.
    NotARepository,
    /// Another git process holds `index.lock`, e.g. one running in the user's terminal.
    IndexLocked,
}

/// A git command that failed, or couldn't start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError {
    pub kind: GitErrorKind,
    pub message: String,
    /// The command's arguments, without the settings the runner adds (see `RunOptions::config`).
    /// Lossily UTF-8, for messages and matching.
    pub args: Vec<String>,
    /// `None` when git couldn't start, or was stopped by a signal.
    pub exit_code: Option<i32>,
    pub stderr: String,
    /// What it printed on stdout, for a command whose output still says something when it fails.
    /// As git printed it, for output that's handed back to git: a patch of a file that isn't
    /// UTF-8, say (TS read such output as Latin-1, which keeps every byte). See `stdout_text`.
    pub stdout: Vec<u8>,
}

impl GitError {
    /// A failure of `args` with `message`, of [`GitErrorKind::Command`].
    pub fn new(
        message: impl Into<String>,
        args: Vec<String>,
        exit_code: Option<i32>,
        stderr: impl Into<String>,
    ) -> Self {
        GitError {
            kind: GitErrorKind::Command,
            message: message.into(),
            args,
            exit_code,
            stderr: stderr.into(),
            stdout: Vec::new(),
        }
    }

    /// `stdout`, lossily UTF-8: for messages and matching.
    pub fn stdout_text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    /// How it reaches the UI: as a bug, unless its kind knows better.
    pub fn api(&self) -> ApiCode {
        match self.kind {
            GitErrorKind::Command => ApiCode::InternalServerError,
            GitErrorKind::NotARepository => ApiCode::NotFound,
            GitErrorKind::IndexLocked => ApiCode::Conflict,
        }
    }

    /// This failure worded as `message`; of the same kind, so it reaches the UI the same way.
    pub fn with_message(&self, message: impl Into<String>) -> GitError {
        GitError {
            message: message.into(),
            ..self.clone()
        }
    }

    /// This failure without git's hints, which suggest commands to type and so don't help in the
    /// app.
    pub fn without_hints(&self) -> GitError {
        let message = self
            .message
            .split('\n')
            .filter(|line| !line.starts_with("hint:"))
            .collect::<Vec<_>>()
            .join("\n");
        let message = message.trim();
        if message.is_empty() || message == self.message {
            self.clone()
        } else {
            self.with_message(message)
        }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitError {}

/// How a file in the working tree changed on disk (see [`Error::FileChangedOnDisk`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskChange {
    Changed,
    Deleted,
}

impl fmt::Display for DiskChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DiskChange::Changed => "changed",
            DiskChange::Deleted => "deleted",
        })
    }
}

/// Whether lines were being staged or unstaged (see [`Error::PatchChanged`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageAction {
    Staged,
    Unstaged,
}

impl fmt::Display for StageAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StageAction::Staged => "staged",
            StageAction::Unstaged => "unstaged",
        })
    }
}

/// Every error the git crate returns. Its `Display` is the message for the user.
///
/// The variants holding only a `String` are errors that are only a message (TS's `messageError`
/// classes): make one with the message, e.g. `Error::NoUpstream(format!(...))`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// A git command that failed, or couldn't start.
    #[error("{0}")]
    Git(GitError),

    /// A pull that stopped partway, at conflicts or a rebase that couldn't go on, and is left for
    /// the user to finish. Not a `Git` error: git may well have succeeded, e.g. stopping a merge
    /// before committing because it was told to.
    #[error("{0}")]
    PullInterrupted(String),

    /// A stash that was popped but conflicts, left for the user to resolve; the stash is kept.
    #[error("{0}")]
    StashConflict(String),

    /// Uncommitted changes that were left in the stash rather than brought along to the branch
    /// switched to: they conflict with it, or stashing them failed partway.
    #[error("{0}")]
    ChangesStashed(String),

    /// There's nothing to pull from: HEAD isn't on a branch, or the branch doesn't track one.
    #[error("{0}")]
    NoUpstream(String),

    /// A branch that isn't checked out but can't be pulled without being checked out: it diverged
    /// from its upstream, or is in use in a worktree. Nothing was done.
    #[error("{0}")]
    PullBlocked(String),

    /// There's nothing to push: HEAD isn't on a branch, the branch has no commits, or there's no
    /// remote.
    #[error("{0}")]
    PushBlocked(String),

    /// A push turned down as the remote branch has commits the branch doesn't. `forcible` when
    /// they're ones the branch had, as before a rebase or an amend, which force-pushing would
    /// replace; not when it never had them, as someone else pushed them.
    #[error("{message}")]
    PushRejected { message: String, forcible: bool },

    /// Git isn't installed (`version` is `None`), or is older than Gitto works with (see
    /// `GitVersion`).
    #[error("{}", unsupported_git_message(.version.as_deref(), .required))]
    UnsupportedGit {
        version: Option<String>,
        required: String,
    },

    /// The repository changed while an operation was under way, which trying again will get past.
    #[error("{0}")]
    RepositoryChanged(String),

    /// HEAD isn't the commit that was meant to be amended, e.g. one was made in a terminal since.
    #[error("The last commit changed before it could be amended, so nothing was amended.")]
    HeadMoved,

    /// A file too large to be read whole, e.g. to show more of it around its changes.
    #[error("This file is {} MB, too large to show more of it.", to_fixed(megabytes(*.bytes), 1))]
    FileTooLarge { bytes: u64 },

    /// A file's changes, too many to send whole: an untracked log file of hundreds of megabytes,
    /// say.
    #[error("These changes are over {} MB, too large to show.", to_fixed(megabytes(*.bytes), 0))]
    ChangesTooLarge { bytes: u64 },

    /// A path that isn't a file inside the repository's working tree, e.g. with `..`, or a link
    /// out.
    #[error("{path} isn't a file in this repository.")]
    OutsideRepository { path: String },

    /// A file that's no longer in the working tree, e.g. deleted since its changes were read.
    #[error("{path} is no longer in the working tree.")]
    WorkingTreeFileNotFound { path: String },

    /// A file whose contents aren't UTF-8, which can't be shown or edited as text.
    #[error("This file isn't UTF-8 text.")]
    NotUtf8,

    /// A file in the working tree that changed on disk since it was read to be edited, which
    /// saving the edits over would lose.
    #[error("{path} was {how} on disk since you started editing it.")]
    FileChangedOnDisk { path: String, how: DiskChange },

    /// A file's changes that aren't what the user picked lines from any more, e.g. as the file was
    /// saved since: staging or unstaging lines of them would move others than the ones they saw.
    #[error("The file changed since its changes were shown, so nothing was {action}.")]
    PatchChanged { action: StageAction },

    /// Lines that can't be staged or unstaged on their own, e.g. of a binary file.
    #[error("{0}")]
    LinesNotStageable(String),

    /// Conflicted files that still have conflict markers, which staging would mark resolved with
    /// the markers in them. `paths` has at least one.
    #[error("{}", conflict_markers_message(.paths))]
    ConflictMarkers { paths: Vec<String> },

    /// A conflict that changed since it was shown, e.g. resolved in a terminal: nothing was done.
    #[error("The conflict in {path} changed since it was shown, so nothing was resolved.")]
    ConflictChanged { path: String },

    /// No merge, rebase, cherry-pick, revert or `git am` is under way, or another one than shown.
    #[error("{0}")]
    NoOperation(String),

    /// A merge that can't start: HEAD isn't on a branch, an operation or conflicts are under way,
    /// the branch to merge is gone, or uncommitted changes or untracked files are in its way.
    /// Nothing was done.
    #[error("{0}")]
    MergeBlocked(String),

    /// A merge that stopped before committing, without conflicts, e.g. as a hook turned the merge
    /// commit down: it's under way, to commit or abort.
    #[error("{0}")]
    MergeStopped(String),

    /// A rebase that can't start: HEAD isn't on a branch, an operation or conflicts are under way,
    /// the branch to rebase onto is gone, or there are uncommitted changes or untracked files in
    /// the way. Nothing was done.
    #[error("{0}")]
    RebaseBlocked(String),

    /// A rebase that stopped partway without conflicts, e.g. as a commit couldn't be signed, and
    /// was undone: nothing is left of it, to try again once that's fixed.
    #[error("{0}")]
    RebaseUndone(String),

    /// A rebase that stopped partway without conflicts, e.g. as a commit couldn't be signed, and
    /// couldn't be undone either: it's under way, to finish in a terminal or abort.
    #[error("{0}")]
    RebaseStopped(String),

    /// Changes that can't be discarded, as they're of conflicted files: nothing was discarded.
    #[error("{0}")]
    DiscardBlocked(String),

    /// A worktree that can't be removed: the main one, which the others share their git directory
    /// with, or the one open here. Nothing was done.
    #[error("{0}")]
    WorktreeRemovalBlocked(String),

    /// A remote branch that can't be deleted with the local branches tracking it, as one of those
    /// is checked out, here or in another worktree. Nothing was deleted.
    #[error("{0}")]
    BranchDeletionBlocked(String),

    /// What's asked of git needs an editor for a message, which Gitto can't open: a rebase's
    /// reword.
    #[error("{0}")]
    EditorNeeded(String),

    /// No repository with that id has been added to Gitto.
    #[error("Repository not found.")]
    RepositoryNotFound { repository_id: String },

    /// Gitto's list of repositories couldn't be read, to find one by its id: a bug, said with why.
    #[error("{0}")]
    RepositoryList(RepositoryListError),

    /// The repository's folder has been moved or deleted.
    #[error("{path} no longer exists.")]
    FolderNotFound { path: String },

    /// Input that isn't valid (see `crate::input`), as zod refused it in TS.
    #[error("{0}")]
    InvalidInput(String),

    /// Something Gitto doesn't expect, e.g. output it can't parse: a bug (TS's plain `Error`).
    #[error("{0}")]
    Bug(String),

    /// A file system failure Gitto doesn't expect: a bug (TS's Node errors, left uncaught).
    #[error("{0}")]
    Io(Arc<io::Error>),
}

impl Error {
    /// How it reaches the UI (see [`ApiCode`]).
    pub fn api(&self) -> ApiCode {
        use ApiCode::*;
        match self {
            Error::Git(error) => error.api(),
            Error::PushRejected { forcible, .. } => {
                // A code of its own, so the UI can offer to force-push, or to pull.
                if *forcible { PushRewritten } else { PushBehind }
            }
            Error::RepositoryNotFound { .. }
            | Error::FolderNotFound { .. }
            | Error::WorkingTreeFileNotFound { .. } => NotFound,
            Error::NoUpstream(_)
            | Error::PullBlocked(_)
            | Error::PushBlocked(_)
            | Error::UnsupportedGit { .. }
            | Error::ConflictMarkers { .. }
            | Error::NoOperation(_)
            | Error::MergeBlocked(_)
            | Error::RebaseBlocked(_)
            | Error::RebaseUndone(_)
            | Error::DiscardBlocked(_)
            | Error::WorktreeRemovalBlocked(_)
            | Error::BranchDeletionBlocked(_)
            | Error::EditorNeeded(_) => PreconditionFailed,
            Error::PullInterrupted(_)
            | Error::StashConflict(_)
            | Error::ChangesStashed(_)
            | Error::RepositoryChanged(_)
            | Error::HeadMoved
            | Error::FileChangedOnDisk { .. }
            | Error::PatchChanged { .. }
            | Error::ConflictChanged { .. }
            | Error::MergeStopped(_)
            | Error::RebaseStopped(_) => Conflict,
            Error::FileTooLarge { .. } | Error::ChangesTooLarge { .. } => PayloadTooLarge,
            Error::OutsideRepository { .. } => Forbidden,
            Error::NotUtf8 => UnsupportedMediaType,
            Error::LinesNotStageable(_) => UnprocessableContent,
            Error::InvalidInput(_) => BadRequest,
            Error::RepositoryList(_) | Error::Bug(_) | Error::Io(_) => InternalServerError,
        }
    }

    /// The git failure this is, if it is one (TS's `error instanceof GitError`).
    pub fn as_git(&self) -> Option<&GitError> {
        match self {
            Error::Git(error) => Some(error),
            _ => None,
        }
    }

    /// Whether this is a git command that exited with `code` (TS's
    /// `error instanceof GitError && error.exitCode === code`).
    pub fn is_exit(&self, code: i32) -> bool {
        self.as_git()
            .is_some_and(|error| error.exit_code == Some(code))
    }

    /// This error without git's hints, if it's a git failure (see [`GitError::without_hints`]);
    /// any other error as it is.
    pub fn without_hints(self) -> Error {
        match self {
            Error::Git(error) => Error::Git(error.without_hints()),
            other => other,
        }
    }
}

impl From<RepositoryListError> for Error {
    fn from(error: RepositoryListError) -> Self {
        Error::RepositoryList(error)
    }
}

impl From<GitError> for Error {
    fn from(error: GitError) -> Self {
        Error::Git(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(Arc::new(error))
    }
}

/// What was being done to Gitto's list of repositories (see [`RepositoryListError`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListAction {
    Read,
    Update,
}

impl fmt::Display for ListAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ListAction::Read => "read",
            ListAction::Update => "update",
        })
    }
}

/// Gitto's list of repositories couldn't be read or changed, e.g. as its database is locked or the
/// disk is full: said with the database's reason. Ported from `packages/repository`'s
/// `RepositoryListError`, which `RepositoryService` rejected with; a [`RepositoryLookup`] returns
/// it when it can't look a repository up, and it reaches the UI with its message, as a bug.
///
/// [`RepositoryLookup`]: crate::core::repo::RepositoryLookup
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Gitto couldn't {action} its list of repositories: {reason}")]
pub struct RepositoryListError {
    pub action: ListAction,
    /// Why, ending in a full stop.
    pub reason: String,
}

impl RepositoryListError {
    /// The list couldn't be `action`ed as `cause` says: by the innermost error it wraps, which is
    /// the database's own (the outer ones wrap it in the query that failed).
    pub fn new(action: ListAction, cause: &(dyn std::error::Error + 'static)) -> Self {
        let mut inner = cause;
        while let Some(source) = inner.source() {
            inner = source;
        }
        RepositoryListError {
            action,
            reason: sentence(&inner.to_string()),
        }
    }
}

/// `message`, trimmed, with a full stop if it has none.
fn sentence(message: &str) -> String {
    let message = message.trim();
    if message.ends_with(['.', '!', '?']) {
        message.to_owned()
    } else {
        format!("{message}.")
    }
}

/// The most specific error for a command that exited with `exit_code`, going by what git printed.
pub fn command_error(
    cwd: &str,
    args: &[String],
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> GitError {
    let lower = stderr.to_lowercase();
    if lower.contains("not a git repository") {
        return GitError {
            kind: GitErrorKind::NotARepository,
            ..GitError::new(
                format!("{cwd} is no longer a git repository."),
                args.to_vec(),
                exit_code,
                stderr,
            )
        };
    }
    if stderr.contains("index.lock") {
        return GitError {
            kind: GitErrorKind::IndexLocked,
            ..GitError::new(
                "Another git process is running in this repository.",
                args.to_vec(),
                exit_code,
                stderr,
            )
        };
    }
    // Some failures, like "nothing to commit", are only explained on stdout.
    let message = match (stderr.trim(), stdout.trim()) {
        ("", "") => format!(
            "git {} exited with code {}",
            args.first().map_or("undefined", String::as_str),
            exit_code.map_or_else(|| "null".to_owned(), |code| code.to_string()),
        ),
        ("", stdout) => stdout.to_owned(),
        (stderr, _) => stderr.to_owned(),
    };
    GitError {
        stdout: stdout.as_bytes().to_vec(),
        ..GitError::new(message, args.to_vec(), exit_code, stderr)
    }
}

fn unsupported_git_message(version: Option<&str>, required: &str) -> String {
    match version {
        None => format!("Gitto couldn't find Git. Install Git {required} or newer."),
        Some(version) => {
            format!("Gitto needs Git {required} or newer, but Git {version} is installed.")
        }
    }
}

fn conflict_markers_message(paths: &[String]) -> String {
    let first = paths.first().map_or("", String::as_str);
    let others = paths.len().saturating_sub(1);
    if others == 0 {
        format!(
            "{first} still has conflict markers. Resolve its conflicts, or mark it resolved with them from its changes if they belong in it."
        )
    } else {
        let files = if others == 1 { "file" } else { "files" };
        format!(
            "{first} and {others} other {files} still have conflict markers. Resolve their conflicts, or mark each resolved with them from its changes if they belong in it."
        )
    }
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

/// `value` with `digits` decimals, rounding halves up as JS's `toFixed` does (Rust's formatting
/// rounds them to even).
pub fn to_fixed(value: f64, digits: i32) -> String {
    let scale = 10f64.powi(digits);
    let rounded = (value * scale).round() / scale;
    format!("{rounded:.prec$}", prec = digits.max(0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn command_errors_are_as_specific_as_git_says() {
        let error = command_error(
            "/repo",
            &args(&["status"]),
            Some(128),
            "",
            "fatal: not a git repository (or any of the parent directories): .git\n",
        );
        assert_eq!(error.kind, GitErrorKind::NotARepository);
        assert_eq!(error.message, "/repo is no longer a git repository.");
        assert_eq!(error.api(), ApiCode::NotFound);

        let error = command_error(
            "/repo",
            &args(&["add", "a"]),
            Some(128),
            "",
            "fatal: Unable to create '/repo/.git/index.lock': File exists.",
        );
        assert_eq!(error.kind, GitErrorKind::IndexLocked);
        assert_eq!(error.api(), ApiCode::Conflict);

        let error = command_error(
            "/repo",
            &args(&["commit"]),
            Some(1),
            "nothing to commit\n",
            "",
        );
        assert_eq!(error.message, "nothing to commit");
        assert_eq!(error.stdout_text(), "nothing to commit\n");
        assert_eq!(error.api(), ApiCode::InternalServerError);

        let error = command_error("/repo", &args(&["diff"]), None, "", "");
        assert_eq!(error.message, "git diff exited with code null");
    }

    #[test]
    fn bugs_are_internal_errors_and_input_the_ui_shouldnt_have_sent() {
        assert!(ApiCode::InternalServerError.is_bug());
        assert!(ApiCode::BadRequest.is_bug());
        for code in [
            ApiCode::NotFound,
            ApiCode::PreconditionFailed,
            ApiCode::Conflict,
            ApiCode::PayloadTooLarge,
            ApiCode::Forbidden,
            ApiCode::UnsupportedMediaType,
            ApiCode::UnprocessableContent,
            ApiCode::PushRewritten,
            ApiCode::PushBehind,
        ] {
            assert!(!code.is_bug(), "{code:?}");
            assert!(code.status() < 500, "{code:?}");
        }
        assert_eq!(ApiCode::PushBehind.status(), 409);
        assert_eq!(ApiCode::PreconditionFailed.status(), 412);
    }

    #[test]
    fn repository_list_errors_say_why_the_database_failed() {
        /// An error wrapping another, as a query's wraps the database's.
        #[derive(Debug)]
        struct Query(io::Error);

        impl fmt::Display for Query {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("Failed query: select * from repositories")
            }
        }

        impl std::error::Error for Query {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let locked = Query(io::Error::other("database is locked"));
        let error = Error::from(RepositoryListError::new(ListAction::Read, &locked));
        assert_eq!(
            error.to_string(),
            "Gitto couldn't read its list of repositories: database is locked."
        );
        assert_eq!(error.api(), ApiCode::InternalServerError);

        let full = io::Error::other("disk is full!");
        assert_eq!(
            RepositoryListError::new(ListAction::Update, &full).to_string(),
            "Gitto couldn't update its list of repositories: disk is full!"
        );
    }

    #[test]
    fn hints_are_left_out() {
        let error = GitError::new(
            "error: could not apply abc\nhint: Resolve all conflicts manually\nhint: then run",
            args(&["rebase"]),
            Some(1),
            "",
        );
        assert_eq!(error.without_hints().message, "error: could not apply abc");
        let only_hints = GitError::new("hint: nothing else", vec![], Some(1), "");
        assert_eq!(only_hints.without_hints().message, "hint: nothing else");
    }

    #[test]
    fn messages_read_as_in_the_app() {
        assert_eq!(
            Error::FileTooLarge {
                bytes: 5 * 1024 * 1024 + 1024 * 1024 / 4
            }
            .to_string(),
            "This file is 5.3 MB, too large to show more of it."
        );
        assert_eq!(
            Error::ChangesTooLarge {
                bytes: 50 * 1024 * 1024
            }
            .to_string(),
            "These changes are over 50 MB, too large to show."
        );
        assert_eq!(
            Error::ConflictMarkers {
                paths: args(&["a", "b", "c"])
            }
            .to_string(),
            "a and 2 other files still have conflict markers. Resolve their conflicts, or mark each resolved with them from its changes if they belong in it."
        );
        assert_eq!(
            Error::UnsupportedGit {
                version: None,
                required: "2.41".into()
            }
            .to_string(),
            "Gitto couldn't find Git. Install Git 2.41 or newer."
        );
        assert_eq!(
            Error::PushRejected {
                message: "no".into(),
                forcible: true
            }
            .api()
            .status(),
            409
        );
    }
}
