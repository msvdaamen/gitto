//! The list of repositories as gitto-git looks repositories up in it: what TS's git router did
//! with `RepositoryService.getRepository`. Rust's own, with no TS file of its own: it's here, not in
//! gitto-git, so gitto-git doesn't depend on the database.

use std::path::PathBuf;

use futures::future::BoxFuture;
use gitto_git::input::RepositoryId;
use gitto_git::{RepositoryListError, RepositoryLookup};

use crate::service::RepositoryService;

/// Finds a repository's folder by its id, so `gitto_git::GitApi::new(service)` opens the
/// repositories added to Gitto. If the list can't be read, the UI is told why, with SQLite's
/// reason, as `RepositoryListError`.
impl RepositoryLookup for RepositoryService {
    fn repository_path<'a>(
        &'a self,
        repository_id: &'a RepositoryId,
    ) -> BoxFuture<'a, Result<Option<PathBuf>, RepositoryListError>> {
        Box::pin(async move {
            let repository = self
                .find(repository_id)
                .await
                .map_err(|error| error.as_repository_list_error().clone())?;
            Ok(repository.map(|repository| PathBuf::from(repository.path)))
        })
    }
}

/// From the database to the status, through the API the UI calls git with.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use gitto_db::Db;
    use gitto_git::features::status::schema::{Head, StatusReply};
    use gitto_git::{ApiCode, ApiError, Error as GitError, GitApi};

    use super::*;
    use crate::store::RepositoryStore;

    struct Fixture {
        service: RepositoryService,
        db: Db,
        api: GitApi,
        dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path().join("gitto-app.db")).unwrap();
        let service = RepositoryService::new(RepositoryStore::new(db.clone()));
        Fixture {
            api: GitApi::new(service.clone()),
            service,
            db,
            dir,
        }
    }

    /// Runs git in `cwd`, which must succeed.
    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    impl Fixture {
        /// A git repository named `name` with one commit on `main` and an untracked file, added
        /// to Gitto; returns its id.
        fn add_repository(&self, name: &str) -> RepositoryId {
            git(self.dir.path(), &["init", "-q", "-b", "main", name]);
            let path = self.dir.path().join(name);
            git(
                &path,
                &[
                    "-c",
                    "user.name=Test User",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "first",
                ],
            );
            fs::write(path.join("new.txt"), "new\n").unwrap();
            smol::block_on(self.service.add_repository(&path))
                .unwrap()
                .id
        }

        /// The status of the repository with `id`, as the UI asks for it.
        fn status(&self, id: &RepositoryId) -> Result<StatusReply, ApiError> {
            smol::block_on(async { self.api.repo(id).await?.status(None).await })
        }
    }

    /// `error`'s code, as the renderer knew it, and its message.
    fn api_error(error: ApiError) -> (&'static str, String) {
        (error.code().code(), error.message().to_owned())
    }

    #[test]
    fn reads_the_status_of_a_repository_added_to_gitto() {
        let f = fixture();
        let id = f.add_repository("gitto");

        let StatusReply::Status(status) = f.status(&id).unwrap() else {
            panic!("expected a full status");
        };

        assert!(
            matches!(&status.summary.head, Head::Branch { name, .. } if name == "main"),
            "{:?}",
            status.summary.head
        );
        let unstaged: Vec<_> = status.changes.unstaged.iter().map(|f| &f.path).collect();
        assert_eq!(unstaged, ["new.txt"]);
    }

    #[test]
    fn says_a_repository_that_isnt_in_the_list_isnt_found() {
        let f = fixture();
        f.add_repository("gitto");
        let missing = RepositoryId::generate();

        let error = f.status(&missing).unwrap_err();

        assert_eq!(error.code(), ApiCode::NotFound);
        assert!(!error.is_bug());
        assert!(
            matches!(error.error(), GitError::RepositoryNotFound { repository_id } if *repository_id == missing),
            "{:?}",
            error.error()
        );
    }

    #[test]
    fn tells_the_ui_why_the_list_of_repositories_couldnt_be_read() {
        let f = fixture();
        let id = f.add_repository("gitto");
        smol::block_on(f.db.run(|conn| conn.execute_batch(r#"drop table "repositories""#)))
            .unwrap();

        let error = f.status(&id).unwrap_err();

        assert!(error.is_bug());
        assert!(matches!(error.error(), GitError::RepositoryList(_)));
        assert_eq!(
            api_error(error),
            (
                "INTERNAL_SERVER_ERROR",
                "Gitto couldn't read its list of repositories: no such table: repositories."
                    .to_owned()
            )
        );
    }
}
