//! Real repositories in a temp directory, for tests that run git. Ported from
//! `packages/git/src/test/fixtures.ts` (and [`conflicts`] from `test/conflicts.ts`).
//!
//! Each test makes its own [`Fixture`]: a temp directory, removed when it's dropped, and the
//! `GitRepos` that opens the repositories made in it by their name. Tests are sync `#[test]`
//! functions that run their async part with `smol::block_on`.

pub mod conflicts;

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, LazyLock, Mutex, Once};
use std::time::{Duration, Instant};

use crate::core::git_dirs::real_path;
use crate::core::repo::{GitRepos, Repo};
use crate::core::runner::{REPOSITORY_ENV, lock};
use crate::core::trace::TARGET;

/// Repositories for one test, in a temp directory of its own.
pub struct Fixture {
    /// Removed when the fixture is dropped.
    _dir: tempfile::TempDir,
    root: PathBuf,
    /// Repository paths by name, for `repos`.
    paths: Arc<Mutex<HashMap<String, PathBuf>>>,
    /// Opens the repositories `create_repo` made (and `add_path` added), by their name.
    pub repos: GitRepos,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    pub fn new() -> Self {
        hermetic_env();
        let dir = tempfile::Builder::new()
            .prefix("gitto-git-")
            .tempdir()
            .expect("a temp directory");
        // Where it really is: git lists worktrees by their real paths, and macOS puts the temp
        // folder under a symlink.
        let root = smol::block_on(real_path(dir.path())).expect("the temp directory's real path");
        let paths = Arc::new(Mutex::new(HashMap::<String, PathBuf>::new()));
        let lookup = Arc::clone(&paths);
        let repos = GitRepos::new(move |id: &str| lock(&lookup).get(id).cloned());
        Fixture {
            _dir: dir,
            root,
            paths,
            repos,
        }
    }

    /// The directory the repositories are made in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Has `repos` open the repository at `path` as `name`, e.g. a linked worktree.
    pub fn add_path(&self, name: &str, path: impl Into<PathBuf>) {
        lock(&self.paths).insert(name.to_owned(), path.into());
    }

    /// Creates an empty repository on `main`, which `repos` opens as `name`, and returns its path.
    pub fn create_repo(&self, name: &str) -> PathBuf {
        git(&self.root, ["init", "-q", "-b", "main", name]);
        self.register(name)
    }

    /// Clones the repository at `from` (which `create_repo` made), with the same author settings,
    /// as `name`; its `main` tracks `from`'s. Returns its path.
    pub fn clone_repo(&self, name: &str, from: &Path) -> PathBuf {
        git(
            &self.root,
            [
                OsStr::new("clone"),
                OsStr::new("-q"),
                from.as_os_str(),
                OsStr::new(name),
            ],
        );
        self.register(name)
    }

    /// An upstream repository with one commit, `first` (adding `a.txt`), and a clone of it that
    /// `repos` opens as `name`. The upstream takes pushes to the branch it has checked out, as a
    /// remote would. Returns the upstream's path, and the clone's.
    pub fn create_clone(&self, name: &str) -> (PathBuf, PathBuf) {
        let upstream = self.create_repo(&format!("{name}-upstream"));
        fs::write(upstream.join("a.txt"), "a\n").unwrap();
        git(&upstream, ["add", "a.txt"]);
        git(&upstream, ["commit", "-qm", "first"]);
        git(
            &upstream,
            ["config", "receive.denyCurrentBranch", "updateInstead"],
        );
        let path = self.clone_repo(name, &upstream);
        (upstream, path)
    }

    /// Sets up the repository `name` in `root` for tests, which `repos` then opens as `name`.
    fn register(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        git(&path, ["config", "user.name", "Test User"]);
        git(&path, ["config", "user.email", "test@example.com"]);
        git(&path, ["config", "commit.gpgsign", "false"]);
        self.add_path(name, &path);
        path
    }

    /// A repository with some history: `first`, then a `side` branch that renames `b.txt` and adds
    /// `s.txt`, merged into `main` (tagged `v1`) after a commit there, plus a commit only a tool's
    /// ref points at. `a file.txt` is modified and `new file.txt` untracked. Opened as `name`.
    pub async fn create_history_repo(&self, name: &str) -> Repo {
        let path = self.create_repo(name);
        fs::write(path.join("a file.txt"), "a\n").unwrap();
        fs::write(path.join("b.txt"), "b\n").unwrap();
        fs::write(path.join("bin.dat"), [0u8, 1, 2]).unwrap();
        git(&path, ["add", "-A"]);
        git(&path, ["commit", "-qm", "first"]);
        git(&path, ["checkout", "-qb", "side"]);
        git(&path, ["mv", "b.txt", "c.txt"]);
        fs::write(path.join("s.txt"), "s\n").unwrap();
        git(&path, ["add", "-A"]);
        git(&path, ["commit", "-qm", "side commit"]);
        git(&path, ["checkout", "-q", "main"]);
        fs::write(path.join("a file.txt"), "a\nmore\n").unwrap();
        git(&path, ["commit", "-qam", "main commit"]);
        git(&path, ["merge", "-q", "side", "-m", "Merge side"]);
        git(&path, ["tag", "v1"]);
        // A commit only reachable from a tool's ref namespace, like T3 Code's checkpoints.
        let checkpoint = git(&path, ["commit-tree", "HEAD^{tree}", "-m", "checkpoint"]);
        git(&path, ["update-ref", "refs/tool/checkpoint", &checkpoint]);
        fs::write(path.join("new file.txt"), "new\n").unwrap();
        fs::write(path.join("a file.txt"), "changed\n").unwrap();
        self.repos.open(name).await.unwrap()
    }
}

/// Makes git in the tests, the runner's and `git`'s, the same on every machine, whoever runs them
/// and from where, once per test run: without the user's own settings (their global and system
/// config: `tag.gpgSign`, `core.hooksPath`, `pull.rebase`, …), nor what the environment says
/// about who commits, or which repository to use (a git hook running `cargo test` sets `GIT_DIR`,
/// say).
fn hermetic_env() {
    static DONE: Once = Once::new();
    DONE.call_once(|| {
        // SAFETY: only tests get here, before their first git command; the standard library
        // reads the environment (to spawn git) under the same lock that this takes.
        unsafe {
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
            std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
            // These override the repos' own `user.name`/`user.email`.
            for key in [
                "GIT_AUTHOR_NAME",
                "GIT_AUTHOR_EMAIL",
                "GIT_AUTHOR_DATE",
                "GIT_COMMITTER_NAME",
                "GIT_COMMITTER_EMAIL",
                "GIT_COMMITTER_DATE",
                // And these how fetches run ssh (see `network_options`).
                "GIT_SSH",
                "GIT_SSH_COMMAND",
                "GIT_SSH_VARIANT",
            ]
            .iter()
            .chain(REPOSITORY_ENV)
            {
                std::env::remove_var(key);
            }
        }
    });
}

/// Runs git in `cwd` and returns its trimmed output (lossily UTF-8); panics if it fails.
pub fn git<I, S>(cwd: impl AsRef<Path>, args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd.as_ref());
    let output = command.output().expect("git runs");
    assert!(
        output.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Records the warnings the crate logs (see `crate::core::trace::TARGET`) from now on: the function
/// returned says which have been, by their message. It's every test's, so a test that checks for a
/// warning has to make it say what only it would.
pub fn logged_warnings() -> impl Fn() -> Vec<String> {
    static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    struct Recorder;

    impl log::Log for Recorder {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn && metadata.target().starts_with(TARGET)
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                lock(&WARNINGS).push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    static RECORDER: Recorder = Recorder;
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        log::set_logger(&RECORDER).expect("no other logger in the tests");
        log::set_max_level(log::LevelFilter::Warn);
    });
    let from = lock(&WARNINGS).len();
    move || lock(&WARNINGS)[from..].to_vec()
}

/// Whether the file system takes a file name that isn't UTF-8, as Linux's do: macOS's APFS
/// refuses one, and Windows names are UTF-16, so the tests of such names are skipped there.
pub fn non_utf8_names() -> bool {
    static PROBED: LazyLock<bool> = LazyLock::new(|| {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let Ok(dir) = tempfile::tempdir() else {
                return false;
            };
            let probe = dir.path().join(OsStr::from_bytes(b"probe-\xe9"));
            fs::write(&probe, "").is_ok() && fs::remove_file(&probe).is_ok()
        }
        #[cfg(not(unix))]
        false
    });
    *PROBED
}

/// Waits until `done` is true, checking every 20ms; panics once `timeout` has passed (Vitest's
/// `vi.waitFor`).
pub async fn wait_for(timeout: Duration, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < timeout, "not done within {timeout:?}");
        async_io::Timer::after(Duration::from_millis(20)).await;
    }
}

/// Writes the executable script `file`, a hook, say.
#[cfg_attr(
    not(unix),
    allow(dead_code, reason = "only Unix-only tests may use it")
)]
pub fn write_script(file: &Path, script: &str) {
    fs::write(file, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::conflicts::{Files, commit_files};
    use super::*;

    #[test]
    fn clones_track_their_upstream() {
        let fx = Fixture::new();
        let (upstream, path) = fx.create_clone("cloned");
        assert_eq!(
            git(&path, ["rev-parse", "--abbrev-ref", "main@{upstream}"]),
            "origin/main"
        );
        assert_eq!(git(&upstream, ["log", "--format=%s"]), "first");
        commit_files(&path, &Files::new().without("a.txt"), "second");
        assert_eq!(git(&path, ["config", "user.name"]), "Test User");
    }

    #[test]
    fn the_history_repo_has_a_merge_and_changes() {
        let fx = Fixture::new();
        let repo = smol::block_on(fx.create_history_repo("history"));
        let path = repo.path();
        assert_eq!(git(path, ["log", "-1", "--format=%s"]), "Merge side");
        assert_eq!(git(path, ["tag"]), "v1");
        assert_eq!(
            git(path, ["status", "--porcelain", "-z"]),
            " M a file.txt\0?? new file.txt\0".trim()
        );
    }

    #[test]
    fn git_has_only_the_repositorys_own_settings() {
        let fx = Fixture::new();
        let path = fx.create_repo("hermetic");
        let settings = git(&path, ["config", "--list", "--show-scope"]);
        assert!(
            settings.lines().all(|line| line.starts_with("local\t")),
            "{settings}"
        );
        assert_eq!(std::env::var_os("GIT_DIR"), None);
    }

    #[test]
    fn non_utf8_names_are_probed_once() {
        assert_eq!(non_utf8_names(), non_utf8_names());
        #[cfg(target_os = "linux")]
        assert!(non_utf8_names());
    }
}
