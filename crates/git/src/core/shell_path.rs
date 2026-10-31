//! The PATH git runs with: the one the user's terminal has. Ported from
//! `apps/electron/src/shell-path.ts`.
//!
//! Opened from the Finder, the Dock or a desktop launcher, an app only gets a minimal PATH (on
//! macOS, `/usr/bin:/bin:/usr/sbin:/sbin`), without what the user's shell startup files add, like
//! Homebrew's `/opt/homebrew/bin`. Git would then be the one in `/usr/bin` rather than the one they
//! installed (Apple's, older than Gitto works with), and what git runs, like git-lfs, a credential
//! helper or gpg, wouldn't be found at all.

use std::ffi::OsString;
#[cfg(unix)]
use std::ffi::{OsStr, c_char};
use std::sync::OnceLock;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use futures::future::FutureExt;
use futures::future::{BoxFuture, Shared};

/// Marks where the PATH starts and ends, among whatever the user's shell startup files print.
#[cfg(unix)]
const MARKER: &str = "_GITTO_SHELL_PATH_";

/// How long the user's shell may take to start before git runs with the PATH Gitto was given.
#[cfg(unix)]
const TIMEOUT: Duration = Duration::from_secs(5);

/// How long what the shell printed is read for once it's exited, if that didn't end with the PATH:
/// a job its startup files left in the background can keep its output open for good.
#[cfg(unix)]
const READ_AFTER_EXIT: Duration = Duration::from_millis(200);

/// The PATH from the user's shell, once `use_path_from_shell` asked for it; `None` in it if the
/// shell couldn't say.
static SHELL_PATH: OnceLock<Shared<BoxFuture<'static, Option<OsString>>>> = OnceLock::new();

/// Has git run with the PATH the user's login shell has, ahead of the one Gitto was given, which
/// is kept as it was if the shell can't be asked. The shell is asked once, in the background, and
/// git commands wait for its answer, for at most 5s. Call it as the app starts; until it's called,
/// git runs with Gitto's own PATH, as in tests. Does nothing on Windows, where apps get the user's
/// PATH.
pub fn use_path_from_shell() {
    #[cfg(unix)]
    SHELL_PATH.get_or_init(|| smol::spawn(path_from_shell()).boxed().shared());
}

/// The PATH to run git with: the one `use_path_from_shell` made, once it's read; `None` for
/// Gitto's own.
pub(crate) async fn git_path() -> Option<OsString> {
    match SHELL_PATH.get() {
        Some(path) => path.clone().await,
        None => None,
    }
}

/// The shell's PATH, then the directories of Gitto's that it doesn't have; `None` if the shell
/// couldn't say.
#[cfg(unix)]
async fn path_from_shell() -> Option<OsString> {
    // On a thread of its own: looking the account up can wait on the network (NSS, LDAP).
    let shell = blocking::unblock(login_shell).await?;
    let path = read_shell_path(&shell, TIMEOUT).await?;
    let path = merge_paths(&path, &std::env::var_os("PATH").unwrap_or_default());
    super::trace::trace(format_args!("git runs with the PATH {}", path.display()));
    Some(path)
}

/// The user's login shell: `$SHELL`, or the one their account has.
#[cfg(unix)]
fn login_shell() -> Option<OsString> {
    if let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
        return Some(shell);
    }
    use std::os::unix::ffi::OsStrExt;
    let mut buffer = vec![0 as c_char; 16 * 1024];
    // SAFETY: `getpwuid_r` writes the entry and its strings into `passwd` and `buffer`, which
    // outlive the reads of them below; `found` is null if there's none.
    unsafe {
        let mut passwd: libc::passwd = std::mem::zeroed();
        let mut found = std::ptr::null_mut();
        let failed = libc::getpwuid_r(
            libc::getuid(),
            &mut passwd,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        );
        if failed != 0 || found.is_null() || passwd.pw_shell.is_null() {
            return None;
        }
        let shell = std::ffi::CStr::from_ptr(passwd.pw_shell);
        Some(OsStr::from_bytes(shell.to_bytes()).to_owned()).filter(|shell| !shell.is_empty())
    }
}

/// Runs `shell` as an interactive login shell, so it reads the same startup files as a terminal,
/// and resolves to the PATH it has; `None` if it fails or takes longer than `timeout`, when it's
/// stopped, with what its startup files started. It runs in a session of its own, so an
/// interactive shell can't take over the terminal Gitto may have been started from.
#[cfg(unix)]
async fn read_shell_path(shell: &OsStr, timeout: Duration) -> Option<OsString> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    use futures_lite::{AsyncReadExt, FutureExt};

    // printenv rather than $PATH, which fish has as a list.
    let script = format!("printf '%s' {MARKER}; printenv PATH; printf '%s' {MARKER}");
    let mut command = std::process::Command::new(shell);
    command
        .arg("-ilc")
        .arg(script)
        // Keeps oh-my-zsh from asking to update itself, which no one would answer.
        .env("DISABLE_AUTO_UPDATE", "true");
    // SAFETY: `setsid` is async-signal-safe, as `pre_exec` needs.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = async_process::Command::from(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Its session is its process group too, numbered after it.
    let group = child.id() as i32;
    let mut stdout = child.stdout.take()?;
    // Until the shell's exited and its output has the PATH, or ends: a job its startup files left
    // in the background (`foo &` in `.zshrc`) keeps that open as long as it runs.
    let read = async {
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        let mut exited = None;
        loop {
            if exited.is_some() && memchr::memmem::find_iter(&output, MARKER).count() >= 2 {
                break;
            }
            let read = async { Some(stdout.read(&mut buffer).await) };
            let read = match exited {
                None => {
                    read.or(async {
                        let _ = child.status().await;
                        None
                    })
                    .await
                }
                Some(at) => {
                    read.or(async {
                        async_io::Timer::at(at + READ_AFTER_EXIT).await;
                        Some(Ok(0))
                    })
                    .await
                }
            };
            match read {
                Some(Ok(0) | Err(_)) => break,
                Some(Ok(read)) => output.extend_from_slice(&buffer[..read]),
                None => exited = Some(Instant::now()),
            }
        }
        let _ = child.status().await;
        Some(output)
    };
    let waited = async {
        async_io::Timer::after(timeout).await;
        None
    };
    let Some(output) = read.or(waited).await else {
        // SAFETY: only signals; fails harmlessly if the group is already gone.
        unsafe {
            libc::kill(-group, libc::SIGTERM);
        }
        return None;
    };
    parse_shell_path(&output)
}

/// The shell's PATH in what it printed, between the markers; `None` if there's none.
#[cfg(unix)]
fn parse_shell_path(output: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStrExt;
    let marker = MARKER.as_bytes();
    let start = memchr::memmem::find(output, marker)?;
    let end = memchr::memmem::rfind(output, marker)?;
    if end <= start {
        return None;
    }
    let path = output[start + marker.len()..end].trim_ascii();
    (!path.is_empty()).then(|| OsStr::from_bytes(path).to_owned())
}

/// `first`'s directories, then those of `rest` that aren't in it.
#[cfg(unix)]
fn merge_paths(first: &OsStr, rest: &OsStr) -> OsString {
    let mut dirs = Vec::new();
    for dir in std::env::split_paths(first).chain(std::env::split_paths(rest)) {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    // Split from PATHs, so none has the separator in it.
    std::env::join_paths(dirs).unwrap_or_else(|_| first.to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_support::write_script;

    #[test]
    fn is_found_among_what_the_startup_files_print() {
        let output = format!("Welcome!\n{MARKER}/a:/b\n{MARKER}\nbye");
        assert_eq!(parse_shell_path(output.as_bytes()).unwrap(), "/a:/b");
        assert_eq!(
            parse_shell_path(b"zsh: command not found: printenv\n"),
            None
        );
        assert_eq!(
            parse_shell_path(format!("{MARKER}\n{MARKER}").as_bytes()),
            None
        );
    }

    #[test]
    fn keeps_each_directory_once_in_order() {
        let merged = merge_paths(OsStr::new("/a:/b::/c"), OsStr::new("/b:/d:/a:/e"));
        assert_eq!(merged, "/a:/b:/c:/d:/e");
    }

    /// A shell at `dir/shell` that runs `script` before what it's asked to, as startup files do.
    fn shell(dir: &tempfile::TempDir, script: &str) -> OsString {
        let shell = dir.path().join("shell");
        write_script(
            &shell,
            &format!("#!/bin/sh\n[ \"$1\" = -ilc ] || exit 1\n{script}\nexec /bin/sh -c \"$2\"\n"),
        );
        shell.into_os_string()
    }

    #[test]
    fn reads_the_path_a_login_shell_has() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell(
            &dir,
            "echo Welcome!\nPATH=\"/opt/homebrew/bin:$PATH\"\nexport PATH",
        );
        let path = smol::block_on(read_shell_path(&shell, TIMEOUT)).unwrap();
        let mut dirs = std::env::split_paths(&path);
        assert_eq!(dirs.next().unwrap(), Path::new("/opt/homebrew/bin"));
    }

    #[test]
    fn doesnt_wait_for_a_job_the_startup_files_left_running() {
        let dir = tempfile::tempdir().unwrap();
        // It keeps the shell's output open after the shell's exited.
        let printing = shell(
            &dir,
            "sleep 3 &\nPATH=\"/opt/homebrew/bin:$PATH\"\nexport PATH",
        );
        let start = Instant::now();
        let path = smol::block_on(read_shell_path(&printing, TIMEOUT)).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        let mut dirs = std::env::split_paths(&path);
        assert_eq!(dirs.next().unwrap(), Path::new("/opt/homebrew/bin"));

        // Nor, for long, when it's not printed the PATH.
        let silent = shell(&dir, "sleep 3 &\nexit 0");
        let start = Instant::now();
        assert_eq!(smol::block_on(read_shell_path(&silent, TIMEOUT)), None);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn is_none_when_the_shell_says_none_or_cant_be_run() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell(&dir, "echo 'zsh: command not found: printenv'\nexit 0");
        assert_eq!(smol::block_on(read_shell_path(&shell, TIMEOUT)), None);
        let missing = dir.path().join("missing").into_os_string();
        assert_eq!(smol::block_on(read_shell_path(&missing, TIMEOUT)), None);
    }

    #[test]
    fn stops_waiting_for_a_shell_that_doesnt_finish() {
        let dir = tempfile::tempdir().unwrap();
        let shell = shell(&dir, "sleep 30");
        let start = Instant::now();
        let timeout = Duration::from_millis(200);
        assert_eq!(smol::block_on(read_shell_path(&shell, timeout)), None);
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
