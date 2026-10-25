//! Runs git, and what's kept per repository around that: the write queue, index refreshes,
//! commit-graphs and the status the UI was last shown.
//!
//! Ported from `packages/git/src/core/runner.ts`. Cancellation: TS's `AbortSignal` is dropping the
//! future. A command's future that's dropped before git exits stops git, and all it started.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, UNIX_EPOCH};

use event_listener::Event;
use futures::future::{BoxFuture, FutureExt, Shared};
use futures_lite::{AsyncReadExt, AsyncWriteExt};

use super::errors::{Error, GitError, Result, command_error};
use super::trace::{trace, tracing};

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
        &out,
        &String::from_utf8_lossy(&err),
    )))
}

/// Runs tasks for a key (a repository's path, say) one at a time, in the order they're asked for:
/// each waits until the ones asked for before it are done, whether they succeeded or not. Its place
/// in line is taken when it's asked for (`run` or `turn` is called), not when it's first polled; a
/// task that's dropped, before its turn or during it, gives its place up.
///
/// The repositories' writes go through one (see `Repo::write`). Reads don't: with
/// GIT_OPTIONAL_LOCKS=0 they take no locks, so they can run alongside anything.
pub struct WriteQueue<K = PathBuf> {
    lines: Mutex<HashMap<K, Arc<Line>>>,
}

impl<K> Default for WriteQueue<K> {
    fn default() -> Self {
        WriteQueue {
            lines: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash> WriteQueue<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `task` once the tasks asked for before it with `key` are done; resolves to its output.
    pub fn run<T, F>(&self, key: K, task: F) -> impl Future<Output = T> + use<K, T, F>
    where
        F: Future<Output = T>,
    {
        let turn = self.turn(key);
        async move {
            let _turn = turn.await;
            task.await
        }
    }

    /// A place in line for `key`, taken now: resolves to a [`Turn`] once the ones before it are
    /// done, which holds the next ones back until it's dropped.
    pub fn turn(&self, key: K) -> impl Future<Output = Turn> + Send + 'static + use<K> {
        let line = Arc::clone(lock(&self.lines).entry(key).or_default());
        let number = {
            let mut state = lock(&line.state);
            let number = state.next;
            state.next += 1;
            number
        };
        let place = Turn { line, number };
        async move {
            loop {
                if place.is_up() {
                    return place;
                }
                let listener = place.line.event.listen();
                if place.is_up() {
                    return place;
                }
                listener.await;
            }
        }
    }
}

/// One key's line of tasks (see `WriteQueue`).
#[derive(Default)]
struct Line {
    state: Mutex<LineState>,
    event: Event,
}

#[derive(Default)]
struct LineState {
    /// The number the next place in line gets.
    next: u64,
    /// The number of the place whose turn it is.
    serving: u64,
    /// Places that were given up before their turn, skipped when it comes.
    given_up: BTreeSet<u64>,
}

/// A place in a `WriteQueue`'s line, whose turn it is once it's been waited for: the next task's
/// turn comes once it's dropped.
pub struct Turn {
    line: Arc<Line>,
    number: u64,
}

impl Turn {
    fn is_up(&self) -> bool {
        lock(&self.line.state).serving == self.number
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut guard = lock(&self.line.state);
        let state = &mut *guard;
        if state.serving == self.number {
            state.serving += 1;
            while state.given_up.remove(&state.serving) {
                state.serving += 1;
            }
            drop(guard);
            self.line.event.notify(usize::MAX);
        } else {
            state.given_up.insert(self.number);
        }
    }
}

/// Writes each repository's commit-graph, once per run of the app. With it, git looks commits up
/// instead of parsing them, which makes sorting the history over all refs, when it needs that (see
/// `getLog`), several times faster: 0.5s instead of 3.4s on vscode. Fresh clones don't have one:
/// git only writes it in gc or maintenance.
///
/// Split, so it only adds the commits that aren't in it yet (in ~30ms when there are none), and
/// not through `WriteQueue`: it doesn't touch the index or refs, and can take seconds the first
/// time, which a commit shouldn't wait for. It's only a cache, so a failure (another git writing
/// it at the same time, a read-only repository) is left for next time.
#[derive(Default)]
pub struct CommitGraphs {
    writes: Mutex<HashMap<PathBuf, Shared<BoxFuture<'static, ()>>>>,
}

impl CommitGraphs {
    /// Writes `path`'s commit-graph if that hasn't been done yet; resolves once it's written. The
    /// write starts now, and runs to its end even if the future is dropped.
    pub fn update(&self, path: &Path) -> impl Future<Output = ()> + Send + 'static + use<> {
        lock(&self.writes)
            .entry(path.to_owned())
            .or_insert_with(|| {
                let write = run_git(
                    path,
                    [
                        "commit-graph",
                        "write",
                        "--reachable",
                        "--split",
                        "--no-progress",
                    ],
                    RunOptions::default(),
                );
                let path = path.to_owned();
                smol::spawn(async move {
                    match write.await {
                        Ok(_) => {
                            trace(format_args!("wrote the commit-graph of {}", path.display()))
                        }
                        Err(error) => trace(format_args!(
                            "couldn't write the commit-graph of {}: {error}",
                            path.display()
                        )),
                    }
                })
                .boxed()
                .shared()
            })
            .clone()
    }
}

/// How long after a write that rewrote files the index is refreshed: git compares the files' and
/// the index's timestamps in whole seconds.
pub const REFRESH_INDEX_AFTER: Duration = Duration::from_millis(1000);

/// A status that takes this long may be reading every file again (see `IndexRefreshes`).
pub const SLOW_STATUS: Duration = Duration::from_millis(500);

/// With this many files the index can't vouch for, a slow first status is put down to them. Fewer
/// are always there after a commit, say, and are read in no time: 0.1ms each in vscode.
pub const MANY_UNSURE_FILES: usize = 1000;

/// Refreshes each repository's index after a write that rewrote files in the working tree, like a
/// checkout. Git can't tell whether a file written in the same second as the index changed since,
/// so it reads it again, in full, whenever it compares the working tree with the index, until the
/// index is written in a later second. Gitto's reads never write it (see `ENV`), so every `status`
/// did: 0.5s instead of 80ms on vscode after switching to a branch 3,000 commits back.
///
/// Through `WriteQueue`, as it takes the index's lock, once a second has passed without another
/// such write. It's only a cache, so a failure (the user's git holding the lock, say) is left
/// alone.
///
/// A fresh clone is in that state too: 19,939 files written in the index's second in vscode, and
/// 1s rather than 25ms for every status. So the index is also refreshed when a repository's first
/// status of this run was slow and the index has many such files. Only then: it holds the lock for
/// as long as that status took, at a moment nobody asked Gitto to write, and a git command run in
/// a terminal meanwhile fails on it. So not for a status that was slow for another reason, like
/// files that aren't in the disk's cache yet, and a checkout made in a terminal later is left to
/// the next write there or here.
pub struct IndexRefreshes {
    writes: Arc<WriteQueue>,
    /// The refresh waiting to run for each repository, by the number it was scheduled as.
    timers: Mutex<HashMap<PathBuf, (u64, smol::Task<()>)>>,
    scheduled: AtomicU64,
    /// The repositories whose first status of this run has been timed.
    timed: Mutex<HashSet<PathBuf>>,
}

impl IndexRefreshes {
    pub fn new(writes: Arc<WriteQueue>) -> Self {
        IndexRefreshes {
            writes,
            timers: Mutex::new(HashMap::new()),
            scheduled: AtomicU64::new(0),
            timed: Mutex::new(HashSet::new()),
        }
    }

    /// Refreshes `path`'s index once `REFRESH_INDEX_AFTER` has passed without another call.
    pub fn schedule(self: &Arc<Self>, path: &Path) {
        let number = self.scheduled.fetch_add(1, Ordering::Relaxed);
        let this = Arc::clone(self);
        let key = path.to_owned();
        let mut timers = lock(&self.timers);
        let timer = smol::spawn(async move {
            async_io::Timer::after(REFRESH_INDEX_AFTER).await;
            {
                let mut timers = lock(&this.timers);
                if timers
                    .get(&key)
                    .is_some_and(|(scheduled, _)| *scheduled == number)
                {
                    // Itself: left to run to its end, rather than cancelled by being dropped.
                    if let Some((_, task)) = timers.remove(&key) {
                        task.detach();
                    }
                }
            }
            // `-q` and `--unmerged`: files that did change, and conflicts, aren't errors.
            let refresh = run_git(
                &key,
                ["update-index", "-q", "--unmerged", "--refresh"],
                RunOptions::default(),
            );
            match this.writes.run(key.clone(), refresh).await {
                Ok(_) => trace(format_args!("refreshed the index of {}", key.display())),
                Err(error) => trace(format_args!(
                    "couldn't refresh the index of {}: {error}",
                    key.display()
                )),
            }
        });
        // The one scheduled before is cancelled by being dropped, once the lock is let go.
        let previous = timers.insert(path.to_owned(), (number, timer));
        drop(timers);
        drop(previous);
    }

    /// Schedules a refresh if this status, which took `took`, is `path`'s first, was slow, and the
    /// index has many files it can't vouch for.
    pub fn status_took(self: &Arc<Self>, path: &Path, took: Duration) {
        if !lock(&self.timed).insert(path.to_owned()) {
            return;
        }
        if took < SLOW_STATUS {
            return;
        }
        let this = Arc::clone(self);
        let path = path.to_owned();
        spawn_detached(async move {
            // The repository is gone, say: nothing to refresh.
            let count = unsure_files(&path).await.unwrap_or(0);
            trace(format_args!(
                "the first status of {} took {}ms, with {count} unsure files",
                path.display(),
                took.as_millis()
            ));
            if count >= MANY_UNSURE_FILES {
                this.schedule(&path);
            }
        });
    }
}

/// How many of the index's files it can't vouch for: the ones it has as last written in the second
/// it was written itself, or later.
async fn unsure_files(path: &Path) -> Result<usize> {
    let (entries, index) = futures::try_join!(
        run_git(path, ["ls-files", "--debug"], RunOptions::default()),
        run_git(
            path,
            ["rev-parse", "--path-format=absolute", "--git-path", "index"],
            RunOptions::default(),
        ),
    )?;
    let modified = async_fs::metadata(index.trim_end()).await?.modified()?;
    let written = modified
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    // Each file's name, then what the index knows of it, indented: `  mtime: <seconds>:<nanos>`.
    let count = entries
        .lines()
        .filter_map(|line| line.strip_prefix("  mtime: "))
        .filter_map(|rest| rest.split_once(':'))
        .filter_map(|(seconds, _)| seconds.parse::<u64>().ok())
        .filter(|seconds| *seconds >= written)
        .count();
    Ok(count)
}

/// A snapshot (see `statusSnapshot`) of the status as the UI got it, or `None` if it didn't.
type Snapshot = Shared<BoxFuture<'static, Option<String>>>;

/// A snapshot (see `statusSnapshot`) of each repository's status as the UI last got it. Git also
/// writes the index when nothing in it changes (another tool's `git status` refreshes it), and
/// after Gitto's own writes the UI already refetches the status itself: comparing the status with
/// the one the UI has tells those apart from a change it hasn't got yet (see `watchGitDir`).
#[derive(Default)]
pub struct ShownStatuses {
    latest: Mutex<HashMap<PathBuf, (u64, Snapshot)>>,
    set: AtomicU64,
}

impl ShownStatuses {
    /// Remembers `snapshot` as `path`'s; it resolves to `None` if the UI didn't get the status.
    pub fn set(
        &self,
        path: &Path,
        snapshot: impl Future<Output = Option<String>> + Send + 'static,
    ) {
        let number = self.set.fetch_add(1, Ordering::Relaxed);
        lock(&self.latest).insert(path.to_owned(), (number, snapshot.boxed().shared()));
    }

    /// `path`'s latest snapshot, once it's there: a status that's being read is waited for.
    pub async fn get(&self, path: &Path) -> Option<String> {
        loop {
            let (number, latest) = lock(&self.latest).get(path).cloned()?;
            let snapshot = latest.await;
            // Whichever is the latest by then.
            if lock(&self.latest).get(path).map(|(set, _)| *set) == Some(number) {
                return snapshot;
            }
        }
    }
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

    #[test]
    fn write_queue_runs_tasks_in_the_order_asked_for() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let order = Mutex::new(Vec::new());
            let first = queue.run("repo", async {
                async_io::Timer::after(Duration::from_millis(50)).await;
                lock(&order).push(1);
            });
            let second = queue.run("repo", async { lock(&order).push(2) });
            let other = queue.run("other", async { lock(&order).push(3) });
            // Polled in the opposite order to the one they were asked for.
            futures::join!(other, second, first);
            assert_eq!(*lock(&order), [3, 1, 2]);
        });
    }

    #[test]
    fn write_queue_skips_tasks_that_were_dropped() {
        smol::block_on(async {
            let queue = WriteQueue::<&str>::new();
            let first = queue.turn("repo");
            let dropped = queue.turn("repo");
            let third = queue.turn("repo");
            drop(dropped);
            let turn = first.await;
            drop(turn);
            third.await;
        });
    }

    #[test]
    fn shown_statuses_wait_for_the_latest() {
        smol::block_on(async {
            let statuses = ShownStatuses::default();
            let path = Path::new("/repo");
            assert_eq!(statuses.get(path).await, None);
            statuses.set(path, async { Some("a".to_owned()) });
            assert_eq!(statuses.get(path).await.as_deref(), Some("a"));
            statuses.set(path, async { None });
            assert_eq!(statuses.get(path).await, None);
        });
    }
}
