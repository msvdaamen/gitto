//! The list of repositories as gitto-git looks repositories up in it: what TS's git router did
//! with `RepositoryService.getRepository`. Rust's own, with no TS file of its own: it's here, not in
//! gitto-git, so gitto-git doesn't depend on the database.

use std::path::PathBuf;

use futures::future::BoxFuture;
use gitto_git::{RepositoryListError, RepositoryLookup};

use crate::errors::{Error, ListAction, reason};
use crate::service::RepositoryService;

/// Finds a repository's folder by its id, so `gitto_git::GitApi::new(service)` opens the
/// repositories added to Gitto. If the list can't be read, the UI is told why, with SQLite's
/// reason, as `RepositoryListError`.
impl RepositoryLookup for RepositoryService {
    fn repository_path<'a>(
        &'a self,
        repository_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<PathBuf>, RepositoryListError>> {
        Box::pin(async move {
            let repository = self
                .get_repository(repository_id)
                .await
                .map_err(|error| list_error(&error))?;
            Ok(repository.map(|repository| PathBuf::from(repository.path)))
        })
    }
}

/// `error` as gitto-git says it: the same message, as both say it as `packages/repository` did.
fn list_error(error: &Error) -> RepositoryListError {
    let action = match error {
        Error::List { action, .. } => *action,
        // Looking a repository up only reads the list, which is all that can fail then.
        Error::NotAGitRepository { .. } => ListAction::Read,
    };
    RepositoryListError {
        action,
        reason: reason(error),
    }
}

/// From the database to the status, through the API the UI calls git with.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use gitto_db::Db;
    use gitto_git::features::status::commands::get_status_since;
    use gitto_git::features::status::schema::{Head, StatusReply};
    use gitto_git::input::RepositoryId;
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
        let db = Db::open(dir.path().join("gitto.db")).unwrap();
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
            let repository = smol::block_on(self.service.add_repository(&path)).unwrap();
            RepositoryId::parse(repository.id).unwrap()
        }

        /// `status.get` of the repository with `id`, as the UI asks for it.
        fn status(&self, id: &RepositoryId) -> Result<StatusReply, ApiError> {
            smol::block_on(self.api.call("status.get", id, |repo| async move {
                get_status_since(&repo, None).await
            }))
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
            matches!(&status.head, Head::Branch { name, .. } if name == "main"),
            "{:?}",
            status.head
        );
        let unstaged: Vec<_> = status.changes.unstaged.iter().map(|f| &f.path).collect();
        assert_eq!(unstaged, ["new.txt"]);
    }

    #[test]
    fn says_a_repository_that_isnt_in_the_list_isnt_found() {
        let f = fixture();
        f.add_repository("gitto");
        let missing = RepositoryId::parse(uuid::Uuid::now_v7().to_string()).unwrap();

        let error = f.status(&missing).unwrap_err();

        assert_eq!(error.code(), ApiCode::NotFound);
        assert!(!error.is_bug());
        assert!(
            matches!(error.error(), GitError::RepositoryNotFound { repository_id } if repository_id == missing.as_str()),
            "{:?}",
            error.error()
        );
    }

    #[test]
    fn refuses_an_id_that_isnt_a_uuid_v7() {
        let f = fixture();
        f.add_repository("gitto");
        let v4 = uuid::Uuid::new_v4().to_string();

        let error = ApiError::from(RepositoryId::parse(v4).unwrap_err());

        assert_eq!(error.code().code(), "BAD_REQUEST");
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

    #[test]
    fn says_list_errors_as_the_repository_service_does() {
        let error = Error::list(
            ListAction::Update,
            gitto_db::rusqlite::Error::SqliteFailure(
                gitto_db::rusqlite::ffi::Error::new(5),
                Some("database is locked".to_owned()),
            ),
        );
        let list = list_error(&error);
        assert_eq!(list.action, ListAction::Update);
        assert_eq!(list.to_string(), error.to_string());
    }
}
