//! What Gitto does with its list of repositories. Ported from
//! `packages/repository/src/server/service.ts`.

use std::path::{Component, Path, PathBuf};

use gitto_db::rusqlite;
use gitto_git::input::RepositoryId;

use crate::errors::{Error, ListAction, ListError, Result};
use crate::store::RepositoryStore;
use crate::types::Repository;

/// The repositories added to Gitto, which it shows as tabs. Cheap to clone; clones share the
/// database. Its futures are executor-agnostic (GPUI's, or smol's): SQLite runs on a thread of
/// its own (see [`gitto_db::Db::run`]).
#[derive(Debug, Clone)]
pub struct RepositoryService {
    store: RepositoryStore,
}

impl RepositoryService {
    pub fn new(store: RepositoryStore) -> Self {
        RepositoryService { store }
    }

    /// In the order of their tabs.
    pub async fn get_repositories(&self) -> Result<Vec<Repository>> {
        listed(ListAction::Read, self.store.get_all().await)
    }

    /// The repository with `id`; `None` if there's none.
    pub async fn get_repository(&self, id: &RepositoryId) -> Result<Option<Repository>> {
        Ok(self.find(id).await?)
    }

    /// The repository with `id`, as `get_repository` has it, failing only as the list can.
    pub(crate) async fn find(&self, id: &RepositoryId) -> Result<Option<Repository>, ListError> {
        self.store
            .get_by_id(id)
            .await
            .map_err(|cause| ListError::new(ListAction::Read, cause))
    }

    /// Adds the git repository at `path` after the others; returns the existing entry if it was
    /// already added. `path` is made absolute as Node's `path.resolve` does (see [`resolve`]), and
    /// is refused with [`Error::NotAGitRepository`] if it has no `.git`, and with
    /// [`Error::PathNotUtf8`] if it isn't UTF-8, as the list keeps paths as text.
    pub async fn add_repository(&self, path: impl AsRef<Path>) -> Result<Repository> {
        let resolved = resolve(path.as_ref());
        let Some(repo_path) = resolved.to_str().map(str::to_owned) else {
            return Err(Error::PathNotUtf8 {
                path: resolved.to_string_lossy().into_owned(),
            });
        };

        let existing = listed(ListAction::Read, self.store.get_by_path(&repo_path).await)?;
        if let Some(existing) = existing {
            return Ok(existing);
        }

        // `.git` is a directory in regular clones and a file in worktrees and submodules.
        let is_repository = async_fs::metadata(Path::new(&repo_path).join(".git"))
            .await
            .is_ok();
        if !is_repository {
            return Err(Error::NotAGitRepository { path: repo_path });
        }

        let repository = Repository {
            id: RepositoryId::generate(),
            name: basename(&repo_path),
            path: repo_path,
        };
        // The one listed at its path: this one, unless it was added meanwhile.
        listed(ListAction::Update, self.store.create(&repository).await)
    }

    /// Removes the repository from Gitto; the folder on disk is left untouched.
    pub async fn remove_repository(&self, id: &RepositoryId) -> Result<()> {
        listed(ListAction::Update, self.store.delete(id).await)
    }

    /// Puts the tabs in the order of `ids`; repositories missing from it keep their place after.
    pub async fn reorder_repositories(&self, ids: &[RepositoryId]) -> Result<()> {
        listed(ListAction::Update, self.store.reorder(ids).await)
    }
}

/// `result` of the list of repositories, failing with why in words (see [`Error::List`]).
fn listed<T>(action: ListAction, result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|cause| Error::list(action, cause))
}

/// `path` made absolute as Node's `path.resolve(path)` does: against the current directory, with
/// `.` and `..` worked out from the text alone (symlinks aren't followed: the path is kept as the
/// user chose it), and without a trailing separator. If the current directory can't be read, where
/// Node would throw, the path is taken as it is.
pub fn resolve(path: &Path) -> PathBuf {
    // `std::path::absolute` refuses an empty path, which Node takes as the current directory.
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    // On Windows this also works out `..`, and a drive-relative path such as `C:repo`, as Node's
    // `path.win32.resolve` does; on Unix, it only joins the current directory.
    let Ok(absolute) = std::path::absolute(path) else {
        return path.to_path_buf();
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            // Past the root it stays the root, as in Node.
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                resolved.push(component)
            }
        }
    }
    resolved
}

/// Node's `path.basename`: the last part of a resolved `path`; empty for a root.
fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gitto_db::Db;

    use super::*;

    struct Fixture {
        service: RepositoryService,
        db: Db,
        dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("gitto.db")).unwrap();
        Fixture {
            service: RepositoryService::new(RepositoryStore::new(db.clone())),
            db,
            dir,
        }
    }

    impl Fixture {
        /// A folder in the temporary directory, with a `.git` folder if `git`.
        fn folder(&self, name: &str, git: bool) -> PathBuf {
            let path = resolve(&self.dir.path().join(name));
            fs::create_dir_all(&path).unwrap();
            if git {
                fs::create_dir(path.join(".git")).unwrap();
            }
            path
        }

        fn add(&self, path: impl AsRef<Path>) -> Result<Repository> {
            smol::block_on(self.service.add_repository(path))
        }

        fn names(&self) -> Vec<String> {
            smol::block_on(self.service.get_repositories())
                .unwrap()
                .into_iter()
                .map(|repository| repository.name)
                .collect()
        }

        /// Runs `sql` on the database, as something else might change it.
        fn execute(&self, sql: &'static str) {
            smol::block_on(self.db.run(move |conn| conn.execute_batch(sql))).unwrap();
        }
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn adds_a_git_repository() {
        let f = fixture();
        let path = f.folder("gitto", true);

        let repository = f.add(&path).unwrap();

        assert_eq!(repository.name, "gitto");
        assert_eq!(repository.path, text(&path));
        assert_eq!(
            smol::block_on(f.service.get_repositories()).unwrap(),
            std::slice::from_ref(&repository)
        );
        assert_eq!(
            smol::block_on(f.service.get_repository(&repository.id)).unwrap(),
            Some(repository)
        );
    }

    #[test]
    fn adds_a_worktree_or_submodule_whose_git_is_a_file() {
        let f = fixture();
        let path = f.folder("worktree", false);
        fs::write(
            path.join(".git"),
            "gitdir: /elsewhere/.git/worktrees/worktree\n",
        )
        .unwrap();

        let repository = f.add(&path).unwrap();

        assert_eq!(repository.name, "worktree");
    }

    #[test]
    fn refuses_a_folder_that_is_not_a_git_repository() {
        let f = fixture();
        let path = f.folder("notes", false);

        let error = f.add(&path).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("{} is not a git repository.", text(&path))
        );
        assert!(matches!(&error, Error::NotAGitRepository { path: p } if *p == text(&path)));
        assert!(!error.is_bug());
        assert!(f.names().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_folder_whose_path_isnt_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let f = fixture();
        let path = f.dir.path().join(OsStr::from_bytes(b"caf\xe9"));

        let error = f.add(&path).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Gitto can't add {}, as its path isn't UTF-8.",
                resolve(&path).to_string_lossy()
            )
        );
        assert!(!error.is_bug());
        assert!(f.names().is_empty());
    }

    #[test]
    fn refuses_a_folder_that_does_not_exist() {
        let f = fixture();
        let path = f.dir.path().join("missing");

        let error = f.add(&path).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("{} is not a git repository.", text(&resolve(&path)))
        );
    }

    #[test]
    fn returns_the_existing_entry_for_a_repository_added_before() {
        let f = fixture();
        let path = f.folder("gitto", true);
        let first = f.add(&path).unwrap();

        // The same folder, spelled another way.
        let again = f.add(path.join("../gitto/.")).unwrap();

        assert_eq!(again, first);
        assert_eq!(f.names(), ["gitto"]);
    }

    #[test]
    fn adds_a_repository_added_twice_at_once_once() {
        let f = fixture();
        let path = f.folder("gitto", true);

        let (first, second) = smol::block_on(futures::future::join(
            f.service.add_repository(&path),
            f.service.add_repository(&path),
        ));

        assert_eq!(first.unwrap(), second.unwrap());
        assert_eq!(f.names(), ["gitto"]);
    }

    #[test]
    fn returns_the_existing_entry_even_if_it_is_no_longer_a_repository() {
        let f = fixture();
        let path = f.folder("gitto", true);
        let first = f.add(&path).unwrap();
        fs::remove_dir(path.join(".git")).unwrap();

        assert_eq!(f.add(&path).unwrap(), first);
    }

    #[test]
    fn adds_repositories_after_the_others() {
        let f = fixture();
        for name in ["one", "two", "three"] {
            f.add(f.folder(name, true)).unwrap();
        }
        assert_eq!(f.names(), ["one", "two", "three"]);
    }

    // Up to the root and down again: on Windows the temporary folder may be on another drive.
    #[cfg(unix)]
    #[test]
    fn resolves_a_relative_path_against_the_current_directory() {
        let f = fixture();
        let cwd = std::env::current_dir().unwrap();
        let path = f.folder("relative", true);
        let normal = |component: &Component<'_>| matches!(component, Component::Normal(_));
        let relative = PathBuf::from_iter(
            cwd.components()
                .filter(normal)
                .map(|_| Component::ParentDir)
                .chain(path.components().filter(normal)),
        );
        assert!(relative.is_relative());

        let repository = f.add(&relative).unwrap();

        assert_eq!(repository.path, text(&path));
    }

    #[cfg(unix)]
    #[test]
    fn keeps_a_symlinked_path_as_it_was_chosen() {
        let f = fixture();
        let target = f.folder("target", true);
        let link = f.dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let repository = f.add(&link).unwrap();

        assert_eq!(repository.name, "link");
        assert_eq!(repository.path, text(&resolve(&link)));
        // Another entry from the folder it links to: paths are compared as they are.
        assert_eq!(f.add(&target).unwrap().name, "target");
        assert_eq!(f.names(), ["link", "target"]);
    }

    #[test]
    fn removes_a_repository_and_leaves_its_folder() {
        let f = fixture();
        let one = f.add(f.folder("one", true)).unwrap();
        let two = f.add(f.folder("two", true)).unwrap();

        smol::block_on(f.service.remove_repository(&one.id)).unwrap();

        assert_eq!(f.names(), ["two"]);
        assert!(Path::new(&one.path).join(".git").is_dir());
        assert_eq!(
            smol::block_on(f.service.get_repository(&one.id)).unwrap(),
            None
        );
        // Removing it again, or an id that was never there, is no error.
        smol::block_on(f.service.remove_repository(&one.id)).unwrap();
        smol::block_on(f.service.remove_repository(&RepositoryId::generate())).unwrap();
        assert_eq!(
            smol::block_on(f.service.get_repository(&two.id)).unwrap(),
            Some(two)
        );
    }

    #[test]
    fn reorders_the_repositories() {
        let f = fixture();
        let ids: Vec<RepositoryId> = ["one", "two", "three"]
            .into_iter()
            .map(|name| f.add(f.folder(name, true)).unwrap().id)
            .collect();

        smol::block_on(
            f.service
                .reorder_repositories(&[ids[2].clone(), ids[0].clone()]),
        )
        .unwrap();

        // `two`, missing from the order, keeps its place after the others.
        assert_eq!(f.names(), ["three", "one", "two"]);
    }

    #[test]
    fn says_why_the_list_could_not_be_read() {
        let f = fixture();
        f.execute(r#"DROP TABLE "repositories""#);

        let error = smol::block_on(f.service.get_repositories()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Gitto couldn't read its list of repositories: no such table: repositories."
        );
        assert!(error.is_bug());
        let error =
            smol::block_on(f.service.get_repository(&RepositoryId::generate())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Gitto couldn't read its list of repositories: no such table: repositories."
        );
        // Looking for it first, adding a repository fails to read the list too.
        let error = f.add(f.folder("gitto", true)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Gitto couldn't read its list of repositories: no such table: repositories."
        );
    }

    #[test]
    fn says_why_the_list_could_not_be_updated() {
        let f = fixture();
        let one = f.add(f.folder("one", true)).unwrap();
        let path = f.folder("two", true);
        // Another connection that holds the write lock, as the Electron app might.
        let other = rusqlite::Connection::open(f.dir.path().join("gitto.db")).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();

        let locked = "Gitto couldn't update its list of repositories: database is locked.";
        assert_eq!(f.add(&path).unwrap_err().to_string(), locked);
        let error = smol::block_on(f.service.remove_repository(&one.id)).unwrap_err();
        assert_eq!(error.to_string(), locked);
        assert!(error.is_bug());
        let error = smol::block_on(
            f.service
                .reorder_repositories(std::slice::from_ref(&one.id)),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), locked);
        // It can still be read meanwhile, in WAL mode.
        assert_eq!(f.names(), ["one"]);

        other.execute_batch("COMMIT").unwrap();
        assert_eq!(f.add(&path).unwrap().name, "two");
    }

    #[test]
    fn says_why_a_repository_could_not_be_added() {
        let f = fixture();
        f.execute(
            r#"CREATE TRIGGER full BEFORE INSERT ON "repositories"
               BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;"#,
        );

        let error = f.add(f.folder("gitto", true)).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Gitto couldn't update its list of repositories: database or disk is full."
        );
        let Error::List(list) = &error else {
            panic!("not a list error: {error:?}");
        };
        assert_eq!(list.action(), ListAction::Update);
        assert_eq!(list.cause().to_string(), "database or disk is full");
    }

    #[test]
    fn resolves_paths_as_node_does() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(resolve(Path::new("")), cwd);
        assert_eq!(resolve(Path::new(".")), cwd);
        assert_eq!(resolve(Path::new("a/./b/../c")), cwd.join("a/c"));
        #[cfg(unix)]
        {
            assert_eq!(resolve(Path::new("/a/b/../c/")), Path::new("/a/c"));
            assert_eq!(resolve(Path::new("//a//b/")), Path::new("/a/b"));
            assert_eq!(resolve(Path::new("/..")), Path::new("/"));
            assert_eq!(resolve(Path::new("/a/../../b")), Path::new("/b"));
            assert_eq!(resolve(Path::new("/")), Path::new("/"));
        }
        #[cfg(windows)]
        {
            assert_eq!(resolve(Path::new(r"C:\a\b\..\c\")), Path::new(r"C:\a\c"));
            assert_eq!(resolve(Path::new("C:/a/b")), Path::new(r"C:\a\b"));
            assert_eq!(resolve(Path::new(r"C:\..")), Path::new(r"C:\"));
        }
    }

    #[test]
    fn names_a_repository_after_its_folder() {
        assert_eq!(basename("/home/me/gitto"), "gitto");
        assert_eq!(basename("/"), "");
    }
}
