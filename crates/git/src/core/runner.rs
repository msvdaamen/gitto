//! Runs git. What's kept per repository around that is in [`queue`](super::queue) (the write
//! queue) and [`caches`](super::caches) (commit-graphs, index refreshes and the status the UI was
//! last shown); re-exported here, where the features take it all from.
//!
//! Ported from `packages/git/src/core/runner.ts`. Cancellation: TS's `AbortSignal` is dropping the
//! future. A command's future that's dropped before git exits stops git, and all it started.

#[cfg(unix)]
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use futures::future::BoxFuture;
use futures_lite::{AsyncReadExt, AsyncWriteExt};

use super::errors::{Error, GitError, Result, command_error};
use super::trace::{trace, tracing};

pub(crate) use super::caches::{CommitGraphs, IndexRefreshes, ShownStatuses};
pub(crate) use super::queue::WriteQueue;

/// How a command is run. Every field is off by default; set them with the builder methods, e.g.
/// `RunOptions::default().stdin(message).rewrites_files()`.
///
/// TS's `signal` is dropping the command's future, and its `binary` is reading the output with a
/// `*_bytes` method (`run_bytes`, `run_git_bytes`) rather than as UTF-8; `stdin` is always bytes.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Written to git's stdin, e.g. a commit message for `commit -F -`.
    pub stdin: Option<Vec<u8>>,
    /// Added to the environment git runs with (see `ENV`), for a command one doesn't suit; over
    /// the process's own and `ENV`.
    pub env: Vec<(OsString, OsString)>,
    /// Settings for this command, e.g. `gc.auto=0`, as with `git -c`.
    pub config: Vec<String>,
    /// Takes paths as globs, as git does by default, rather than literally (see `ENV`): for a
    /// command that takes no paths from the UI but uses pathspecs of its own, like `stash push`.
    pub glob_pathspecs: bool,
    /// The command rewrites files in the working tree, like a checkout: the repository's index is
    /// refreshed a little after it's done, or has failed partway, or was cancelled (see
    /// `IndexRefreshes`). Only through a `GitCommand` (a `Repo`'s); `run_git` has no repository
    /// state to refresh.
    pub rewrites_files: bool,
    /// Stopped (with all it started, like ssh) when Gitto exits, rather than left running in its
    /// session (see `run_git`), out of reach: for a command that's safe to stop partway, like a
    /// fetch. Others, like a rebase, aren't stopped on purpose, though one that writes output once
    /// Gitto has gone stops then, its pipe closed. Without a terminal, none waits on an answer that
    /// won't come, so even left behind (when Gitto is stopped by a signal, say) they end.
    pub stop_on_exit: bool,
    /// Stops git once its output is over `bytes`, and fails with `error`: for output that's sent
    /// to the UI whole, like a file's patch, which can be of a file of any size.
    pub max_output: Option<MaxOutput>,
}

/// See [`RunOptions::max_output`].
#[derive(Debug, Clone)]
pub struct MaxOutput {
    pub bytes: usize,
    pub error: Error,
}

impl RunOptions {
    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn config(mut self, setting: impl Into<String>) -> Self {
        self.config.push(setting.into());
        self
    }

    pub fn glob_pathspecs(mut self) -> Self {
        self.glob_pathspecs = true;
        self
    }

    pub fn rewrites_files(mut self) -> Self {
        self.rewrites_files = true;
        self
    }

    pub fn stop_on_exit(mut self) -> Self {
        self.stop_on_exit = true;
        self
    }

    pub fn max_output(mut self, bytes: usize, error: Error) -> Self {
        self.max_output = Some(MaxOutput { bytes, error });
        self
    }
}

/// Settings that keep git's output stable and machine-readable, whatever the user's config says.
pub const CONFIG: &[&str] = &[
    "-c",
    "core.quotepath=false",
    "-c",
    "color.ui=false",
    "-c",
    "log.showSignature=false",
    // Text is read and written in UTF-8, e.g. commit messages, as Rust strings are.
    "-c",
    "i18n.commitEncoding=UTF-8",
    "-c",
    "i18n.logOutputEncoding=UTF-8",
];

/// The environment git runs with, over the process's own.
pub const ENV: &[(&str, &str)] = &[
    // Parse-stable, English output.
    ("LC_ALL", "C"),
    // Never block on a tty prompt; there's no terminal to answer it.
    ("GIT_TERMINAL_PROMPT", "0"),
    // Read commands (status, diff) won't refresh the index, so they never take index.lock and
    // collide with git running in the user's terminal.
    ("GIT_OPTIONAL_LOCKS", "0"),
    // Paths from the UI are file names, not globs.
    ("GIT_LITERAL_PATHSPECS", "1"),
    ("GIT_PAGER", "cat"),
];

/// Locks `mutex`, whether or not a thread panicked holding it: what's kept behind these stays
/// consistent between statements.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `future` to its end in the background, for work nobody waits on (TS's `void promise`),
/// like refreshing the index: futures only run while polled, so one that's only started and
/// dropped wouldn't run at all. On smol's global executor, so it doesn't matter which executor
/// (GPUI's, say) the caller runs on.
pub fn spawn_detached(future: impl Future<Output = ()> + Send + 'static) {
    smol::spawn(future).detach();
}

/// Arguments, as the runner takes them.
pub(crate) fn os_args<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect()
}

/// The process groups of the commands to stop when Gitto exits (see `RunOptions::stop_on_exit`).
#[cfg(unix)]
static STOP_ON_EXIT: Mutex<Option<HashSet<i32>>> = Mutex::new(None);

/// Stops the commands that are to be stopped when Gitto exits (see `RunOptions::stop_on_exit`).
/// Done by itself when the process exits normally (`exit`, or returning from `main`); the app can
/// also call it as it quits.
pub fn stop_commands_on_exit() {
    #[cfg(unix)]
    for group in lock(&STOP_ON_EXIT).take().unwrap_or_default() {
        stop_group(group);
    }
}

#[cfg(unix)]
extern "C" fn stop_commands_at_exit() {
    stop_commands_on_exit();
}

#[cfg(unix)]
fn register_stop_on_exit(group: i32) {
    static AT_EXIT: std::sync::Once = std::sync::Once::new();
    // SAFETY: registers a plain function that only signals processes.
    AT_EXIT.call_once(|| unsafe {
        libc::atexit(stop_commands_at_exit);
    });
    lock(&STOP_ON_EXIT)
        .get_or_insert_with(HashSet::new)
        .insert(group);
}

#[cfg(unix)]
fn unregister_stop_on_exit(group: i32) {
    if let Some(groups) = lock(&STOP_ON_EXIT).as_mut() {
        groups.remove(&group);
    }
}

/// Stops the process group `group`, if it's still there.
#[cfg(unix)]
fn stop_group(group: i32) {
    // SAFETY: only signals; fails harmlessly if the group is already gone.
    unsafe {
        libc::kill(-group, libc::SIGTERM);
    }
}

/// Git's process group, stopped if the command's future is dropped before git exits, and kept in
/// `STOP_ON_EXIT` meanwhile if it's to be stopped on exit.
#[cfg(unix)]
struct Group {
    id: i32,
    stop_on_exit: bool,
    exited: bool,
}

#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        if self.stop_on_exit {
            unregister_stop_on_exit(self.id);
        }
        // Cancelled: git, and what it started, are stopped with its group. SIGTERM, which git
        // cleans its lock files up on.
        if !self.exited {
            stop_group(self.id);
        }
    }
}

/// Runs `git` in `cwd` and resolves to its stdout, decoded as UTF-8 (lossily); fails with a
/// `GitError` (of the most specific kind, see `command_error`) on a non-zero exit. See
/// [`run_git_bytes`].
pub fn run_git<I, S>(
    cwd: impl AsRef<Path>,
    args: I,
    options: RunOptions,
) -> BoxFuture<'static, Result<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git_bytes(cwd, args, options);
    Box::pin(async move { Ok(String::from_utf8_lossy(&output.await?).into_owned()) })
}

/// Runs `git` in `cwd` and resolves to its stdout as bytes, for paths that are handed back to git
/// as it gave them: a file's name needn't be UTF-8, and one that isn't doesn't survive being read
/// as it. Fails with a `GitError` (of the most specific kind, see `command_error`) on a non-zero
/// exit. Dropping the future stops git, with all it started.
///
/// Git runs in a session of its own, without the terminal Gitto may have been started from, so
/// nothing it starts can ask questions there that no one would answer and hold up the repository
/// meanwhile: ssh can't ask for a passphrase or to trust a host (it asks with the user's askpass
/// program instead, if they have one, or fails), nor gpg for one to sign with.
/// GIT_TERMINAL_PROMPT only covers git's own HTTPS prompts. Not on Windows, which has no such
/// terminal to keep away from.
///
/// The arguments are taken when it's called, and git started when the future is first polled.
pub fn run_git_bytes<I, S>(
    cwd: impl AsRef<Path>,
    args: I,
    options: RunOptions,
) -> BoxFuture<'static, Result<Vec<u8>>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Box::pin(run(cwd.as_ref().to_owned(), os_args(args), options))
}

async fn run(cwd: PathBuf, args: Vec<OsString>, options: RunOptions) -> Result<Vec<u8>> {
    let start = tracing().then(Instant::now);
    let shown_args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    let mut command = std::process::Command::new("git");
    command.args(CONFIG);
    for setting in &options.config {
        command.arg("-c").arg(setting);
    }
    command
        .args(&args)
        .current_dir(&cwd)
        .envs(ENV.iter().copied());
    if options.glob_pathspecs {
        command.env("GIT_LITERAL_PATHSPECS", "0");
    }
    command.envs(options.env.iter().map(|(key, value)| (key, value)));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe, as `pre_exec` needs.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut command = async_process::Command::from(command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // On Unix the group is stopped instead (see `Group`), with SIGTERM rather than SIGKILL.
    #[cfg(not(unix))]
    command.kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| {
        let message = if error.kind() == std::io::ErrorKind::NotFound {
            "Git is not installed or not on the PATH.".to_owned()
        } else {
            error.to_string()
        };
        Error::Git(GitError::new(message, shown_args.clone(), None, ""))
    })?;

    // Its own session is its own process group too, numbered after it.
    #[cfg(unix)]
    let mut group = {
        let id = child.id() as i32;
        if options.stop_on_exit {
            register_stop_on_exit(id);
        }
        Group {
            id,
            stop_on_exit: options.stop_on_exit,
            exited: false,
        }
    };

    let mut stdin = child.stdin.take();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let max_output = options.max_output.as_ref().map(|max| max.bytes);

    let write_stdin = async {
        if let (Some(pipe), Some(input)) = (stdin.as_mut(), options.stdin.as_deref()) {
            // Git can exit before reading all of stdin, e.g. when the index is locked; writing the
            // rest then fails with EPIPE. Its exit code and stderr already say why, so that's not
            // reported.
            let _ = pipe.write_all(input).await;
        }
        drop(stdin.take());
    };
    let read_stdout = async {
        let mut out = Vec::new();
        let mut too_large = false;
        if let Some(pipe) = stdout.as_mut() {
            let mut buffer = vec![0; 64 * 1024];
            loop {
                let read = match pipe.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                if too_large {
                    continue;
                }
                if max_output.is_some_and(|max| out.len() + read > max) {
                    too_large = true;
                    out = Vec::new();
                    // With what it started, like a textconv filter, as when it's cancelled.
                    #[cfg(unix)]
                    stop_group(group.id);
                    // Without groups, git is stopped once its output is no longer read.
                    #[cfg(not(unix))]
                    break;
                    #[cfg(unix)]
                    continue;
                }
                out.extend_from_slice(&buffer[..read]);
            }
        }
        drop(stdout.take());
        (out, too_large)
    };
    let read_stderr = async {
        let mut err = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut err).await;
        }
        err
    };
    let ((), (out, too_large), err) = futures::join!(write_stdin, read_stdout, read_stderr);

    #[cfg(not(unix))]
    if too_large {
        let _ = child.kill();
    }
    let status = child.status().await;
    #[cfg(unix)]
    {
        group.exited = true;
    }
    let code = match status {
        Ok(status) => status.code(),
        Err(error) => {
            return Err(Error::Git(GitError::new(
                error.to_string(),
                shown_args,
                None,
                "",
            )));
        }
    };

    if let Some(start) = start {
        trace(format_args!(
            "{:>5}ms {:>8}B  git {}",
            start.elapsed().as_millis(),
            out.len(),
            shown_args.join(" "),
        ));
    }
    if too_large && let Some(max) = options.max_output {
        return Err(max.error);
    }
    if code == Some(0) {
        return Ok(out);
    }
    Err(Error::Git(command_error(
        &cwd.to_string_lossy(),
        &shown_args,
        code,
        &String::from_utf8_lossy(&out),
        &String::from_utf8_lossy(&err),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Fixture, git, non_utf8_names};

    #[test]
    fn writes_stdin() {
        let fx = Fixture::new();
        let path = fx.create_repo("stdin");
        let sha = smol::block_on(run_git(
            &path,
            ["hash-object", "--stdin"],
            RunOptions::default().stdin("a\n"),
        ))
        .unwrap();
        // The blob of `a\n`.
        assert_eq!(sha.trim(), "78981922613b2afb6025042ff6bd878ac1994e85");
    }

    #[test]
    fn takes_paths_literally_unless_asked_to_glob() {
        let fx = Fixture::new();
        let path = fx.create_repo("globs");
        std::fs::write(path.join("a.txt"), "a\n").unwrap();
        git(&path, ["add", "a.txt"]);
        smol::block_on(async {
            let literal = run_git(&path, ["ls-files", "--", "*.txt"], RunOptions::default());
            assert_eq!(literal.await.unwrap(), "");
            let globbed = run_git(
                &path,
                ["ls-files", "--", "*.txt"],
                RunOptions::default().glob_pathspecs(),
            );
            assert_eq!(globbed.await.unwrap(), "a.txt\n");
        });
    }

    #[test]
    fn runs_with_the_environment_its_given() {
        let fx = Fixture::new();
        let path = fx.create_repo("env");
        let ident = smol::block_on(run_git(
            &path,
            ["var", "GIT_AUTHOR_IDENT"],
            RunOptions::default().env("GIT_AUTHOR_NAME", "Someone Else"),
        ))
        .unwrap();
        assert!(
            ident.starts_with("Someone Else <test@example.com>"),
            "{ident}"
        );
    }

    #[test]
    fn stops_git_once_its_output_is_too_large() {
        let fx = Fixture::new();
        let path = fx.create_repo("large");
        std::fs::write(path.join("large.txt"), "x".repeat(1024 * 1024)).unwrap();
        git(&path, ["add", "large.txt"]);
        smol::block_on(async {
            let options =
                RunOptions::default().max_output(1000, Error::ChangesTooLarge { bytes: 1000 });
            let error = run_git(&path, ["cat-file", "-p", ":large.txt"], options.clone())
                .await
                .unwrap_err();
            assert!(matches!(error, Error::ChangesTooLarge { .. }), "{error:?}");
            let small = run_git(&path, ["rev-parse", "HEAD:"], options).await;
            assert!(small.unwrap_err().is_exit(128));
        });
    }

    #[test]
    fn reads_output_as_bytes() {
        if !non_utf8_names() {
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let fx = Fixture::new();
            let path = fx.create_repo("bytes");
            let name = OsStr::from_bytes(b"caf\xe9.txt");
            std::fs::write(path.join(name), "").unwrap();
            git(&path, [OsStr::new("add"), OsStr::new("--"), name]);
            smol::block_on(async {
                let listed = run_git_bytes(&path, ["ls-files", "-z"], RunOptions::default());
                assert_eq!(listed.await.unwrap(), b"caf\xe9.txt\0");
                // Handed back to git as it gave it.
                let args = [OsStr::new("ls-files"), OsStr::new("--error-unmatch"), name];
                assert!(
                    run_git_bytes(&path, args, RunOptions::default())
                        .await
                        .is_ok()
                );
            });
        }
    }

    #[test]
    fn says_why_git_failed() {
        let fx = Fixture::new();
        let path = fx.create_repo("failing");
        let error = smol::block_on(run_git(&path, ["commit"], RunOptions::default())).unwrap_err();
        let git = error.as_git().unwrap();
        assert_eq!(git.exit_code, Some(1));
        assert!(git.message.contains("nothing to commit"), "{}", git.message);
        assert_eq!(
            error.api(),
            crate::core::errors::ApiCode::InternalServerError
        );
    }
}
