//! The list of repositories in the database: the `repositories` table, which the migrations make.
//! Ported from `packages/repository/src/server/store.ts` and its table (`schema.ts`); the SQL is
//! what Drizzle made of TS's queries.

use gitto_db::Db;
use gitto_db::rusqlite::types::Type;
use gitto_db::rusqlite::{self, Connection, OptionalExtension, Row, params};
use gitto_git::input::RepositoryId;

use crate::types::Repository;

/// A repository's columns, without where it's listed.
const COLUMNS: &str = r#""id", "name", "path""#;

/// Reads and changes the list of repositories, each on a thread of its own (see [`Db::run`]).
/// Cheap to clone; clones share the database.
#[derive(Debug, Clone)]
pub struct RepositoryStore {
    db: Db,
}

impl RepositoryStore {
    pub fn new(db: Db) -> Self {
        RepositoryStore { db }
    }

    /// Every repository, in the order of their tabs.
    pub async fn get_all(&self) -> rusqlite::Result<Vec<Repository>> {
        self.db.run(|conn| get_all(conn)).await
    }

    pub async fn get_by_id(&self, id: &RepositoryId) -> rusqlite::Result<Option<Repository>> {
        let id = id.to_string();
        self.db.run(move |conn| get_by(conn, "id", &id)).await
    }

    pub async fn get_by_path(&self, path: &str) -> rusqlite::Result<Option<Repository>> {
        let path = path.to_owned();
        self.db.run(move |conn| get_by(conn, "path", &path)).await
    }

    /// Adds `repository` after the others.
    pub async fn create(&self, repository: &Repository) -> rusqlite::Result<()> {
        let repository = repository.clone();
        self.db.run(move |conn| create(conn, &repository)).await
    }

    pub async fn delete(&self, id: &RepositoryId) -> rusqlite::Result<()> {
        let id = id.to_string();
        self.db.run(move |conn| delete(conn, &id)).await
    }

    /// Puts the repositories in the order of `ids`; ones missing from it keep their place after.
    pub async fn reorder(&self, ids: &[RepositoryId]) -> rusqlite::Result<()> {
        let ids: Vec<String> = ids.iter().map(RepositoryId::to_string).collect();
        self.db.run(move |conn| reorder(conn, &ids)).await
    }
}

/// A repository's row. Ids are written hyphenated and in lower case, but read in either case, as
/// they're the Electron app's too; one that isn't a repository id fails, as it's no row of Gitto's.
fn repository(row: &Row<'_>) -> rusqlite::Result<Repository> {
    let id: String = row.get(0)?;
    let id = RepositoryId::parse(id).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
    })?;
    Ok(Repository {
        id,
        name: row.get(1)?,
        path: row.get(2)?,
    })
}

fn get_all(conn: &Connection) -> rusqlite::Result<Vec<Repository>> {
    conn.prepare(&format!(
        r#"select {COLUMNS} from "repositories" order by "repositories"."position" asc"#
    ))?
    .query_map([], repository)?
    .collect()
}

/// The repository whose `column` is `value`, if there's one. `column` goes into the SQL as it is,
/// so it's only ever a literal.
fn get_by(
    conn: &Connection,
    column: &'static str,
    value: &str,
) -> rusqlite::Result<Option<Repository>> {
    conn.query_row(
        &format!(
            r#"select {COLUMNS} from "repositories" where "repositories"."{column}" = ? limit ?"#
        ),
        params![value, 1],
        repository,
    )
    .optional()
}

fn create(conn: &Connection, repository: &Repository) -> rusqlite::Result<()> {
    conn.execute(
        r#"insert into "repositories" ("id", "name", "path", "position") values (?, ?, ?, (SELECT coalesce(max("repositories"."position"), -1) + 1 FROM "repositories"))"#,
        params![repository.id.to_string(), repository.name, repository.path],
    )?;
    Ok(())
}

fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        r#"delete from "repositories" where "repositories"."id" = ?"#,
        [id],
    )?;
    Ok(())
}

fn reorder(conn: &mut Connection, ids: &[String]) -> rusqlite::Result<()> {
    // Past the end of `ids`, so any repository missing from it lands after the ones in it.
    let rest = ids.len() as i64;
    // `begin`, as Drizzle: deferred.
    let tx = conn.transaction()?;
    tx.execute(
        r#"update "repositories" set "position" = "repositories"."position" + ?"#,
        [rest],
    )?;
    for (position, id) in ids.iter().enumerate() {
        tx.execute(
            r#"update "repositories" set "position" = ? where "repositories"."id" = ?"#,
            params![position as i64, id],
        )?;
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> RepositoryStore {
        RepositoryStore::new(Db::open_in_memory().unwrap())
    }

    /// The id of the repository named `name` in these tests: its first six bytes, in the id's last
    /// part, so ids sort as names do.
    fn id(name: &str) -> RepositoryId {
        let node = name
            .bytes()
            .take(6)
            .fold(0u64, |node, byte| node << 8 | u64::from(byte));
        RepositoryId::parse(format!("01900000-0000-7000-8000-{node:012x}")).unwrap()
    }

    /// The name of the repository with `id` (see [`id`]).
    fn name_of(id: &str) -> String {
        let node = u64::from_str_radix(&id[24..], 16).unwrap();
        let bytes: Vec<u8> = node
            .to_be_bytes()
            .into_iter()
            .skip_while(|&b| b == 0)
            .collect();
        String::from_utf8(bytes).unwrap()
    }

    fn repository(name: &str) -> Repository {
        Repository {
            id: id(name),
            name: format!("{name}-name"),
            path: format!("/repos/{name}"),
        }
    }

    /// The repositories' names, in the order of the tabs.
    fn ids(store: &RepositoryStore) -> Vec<String> {
        smol::block_on(store.get_all())
            .unwrap()
            .into_iter()
            .map(|repository| name_of(&repository.id.to_string()))
            .collect()
    }

    /// Each repository's name and position, by id.
    fn positions(store: &RepositoryStore) -> Vec<(String, i64)> {
        smol::block_on(store.db.run(|conn| {
            conn.prepare(r#"select "id", "position" from "repositories" order by "id""#)?
                .query_map([], |row| {
                    Ok((name_of(&row.get::<_, String>(0)?), row.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()
        }))
        .unwrap()
    }

    fn store_with(names: &[&str]) -> RepositoryStore {
        let store = store();
        for name in names {
            smol::block_on(store.create(&repository(name))).unwrap();
        }
        store
    }

    /// The ids of the repositories named `names`.
    fn ids_of(names: &[&str]) -> Vec<RepositoryId> {
        names.iter().map(|name| id(name)).collect()
    }

    #[test]
    fn lists_no_repositories_at_first() {
        assert_eq!(smol::block_on(store().get_all()).unwrap(), []);
    }

    #[test]
    fn adds_each_repository_after_the_others() {
        let store = store_with(&["a", "b", "c"]);
        assert_eq!(ids(&store), ["a", "b", "c"]);
        assert_eq!(
            positions(&store),
            [("a".into(), 0), ("b".into(), 1), ("c".into(), 2)]
        );
        assert_eq!(
            smol::block_on(store.get_all()).unwrap(),
            [repository("a"), repository("b"), repository("c")]
        );
    }

    #[test]
    fn adds_after_the_last_tab_when_others_were_removed() {
        let store = store_with(&["a", "b", "c"]);
        smol::block_on(store.delete(&id("c"))).unwrap();
        smol::block_on(store.delete(&id("a"))).unwrap();
        smol::block_on(store.create(&repository("d"))).unwrap();
        // One past the highest position, gaps and all.
        assert_eq!(positions(&store), [("b".into(), 1), ("d".into(), 2)]);
        assert_eq!(ids(&store), ["b", "d"]);
    }

    #[test]
    fn gets_a_repository_by_id() {
        let store = store_with(&["a", "b"]);
        assert_eq!(
            smol::block_on(store.get_by_id(&id("b"))).unwrap(),
            Some(repository("b"))
        );
        assert_eq!(
            smol::block_on(store.get_by_id(&id("missing"))).unwrap(),
            None
        );
    }

    #[test]
    fn reads_ids_in_lower_case_and_refuses_ones_that_arent_repository_ids() {
        let store = store();
        smol::block_on(store.db.run(|conn| {
            conn.execute_batch(
                r#"insert into "repositories" ("id", "name", "path", "position")
                   values ('01900000-0000-7000-8000-00000000000A', 'a', '/repos/a', 0)"#,
            )
        }))
        .unwrap();
        let all = smol::block_on(store.get_all()).unwrap();
        assert_eq!(
            all[0].id.to_string(),
            "01900000-0000-7000-8000-00000000000a"
        );

        smol::block_on(store.db.run(|conn| {
            conn.execute_batch(
                r#"insert into "repositories" ("id", "name", "path", "position")
                   values ('b', 'b', '/repos/b', 1)"#,
            )
        }))
        .unwrap();
        let error = smol::block_on(store.get_all()).unwrap_err();
        assert!(
            error.to_string().contains("Not a repository id. (\"b\")"),
            "{error}"
        );
    }

    #[test]
    fn gets_a_repository_by_path() {
        let store = store_with(&["a", "b"]);
        assert_eq!(
            smol::block_on(store.get_by_path("/repos/a")).unwrap(),
            Some(repository("a"))
        );
        // Exactly that path.
        assert_eq!(
            smol::block_on(store.get_by_path("/repos/a/")).unwrap(),
            None
        );
        assert_eq!(smol::block_on(store.get_by_path("/REPOS/A")).unwrap(), None);
    }

    #[test]
    fn refuses_a_second_repository_with_the_same_path() {
        let store = store_with(&["a"]);
        let same_path = Repository {
            id: id("other"),
            ..repository("a")
        };
        let error = smol::block_on(store.create(&same_path)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "UNIQUE constraint failed: repositories.path"
        );
        assert_eq!(ids(&store), ["a"]);
    }

    #[test]
    fn refuses_a_second_repository_with_the_same_id() {
        let store = store_with(&["a"]);
        let same_id = Repository {
            path: "/elsewhere".to_owned(),
            ..repository("a")
        };
        let error = smol::block_on(store.create(&same_id)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "UNIQUE constraint failed: repositories.id"
        );
    }

    #[test]
    fn deletes_a_repository() {
        let store = store_with(&["a", "b", "c"]);
        smol::block_on(store.delete(&id("b"))).unwrap();
        assert_eq!(ids(&store), ["a", "c"]);
        // An id that isn't there is no error.
        smol::block_on(store.delete(&id("missing"))).unwrap();
        assert_eq!(ids(&store), ["a", "c"]);
    }

    #[test]
    fn reorders_the_repositories() {
        let store = store_with(&["a", "b", "c"]);
        smol::block_on(store.reorder(&ids_of(&["c", "a", "b"]))).unwrap();
        assert_eq!(ids(&store), ["c", "a", "b"]);
        assert_eq!(
            positions(&store),
            [("a".into(), 1), ("b".into(), 2), ("c".into(), 0)]
        );
    }

    #[test]
    fn keeps_the_place_of_repositories_missing_from_the_order_after_the_others() {
        let store = store_with(&["a", "b", "c", "d"]);
        smol::block_on(store.reorder(&ids_of(&["d", "b"]))).unwrap();
        // a and c keep their order, after the two in the list.
        assert_eq!(ids(&store), ["d", "b", "a", "c"]);
        assert_eq!(
            positions(&store),
            [
                ("a".into(), 2),
                ("b".into(), 1),
                ("c".into(), 4),
                ("d".into(), 0)
            ]
        );
    }

    #[test]
    fn leaves_the_order_as_it_is_without_ids() {
        let store = store_with(&["a", "b"]);
        smol::block_on(store.reorder(&[])).unwrap();
        assert_eq!(positions(&store), [("a".into(), 0), ("b".into(), 1)]);
    }

    #[test]
    fn ignores_ids_that_are_not_in_the_list() {
        let store = store_with(&["a", "b"]);
        smol::block_on(store.reorder(&ids_of(&["missing", "b", "a"]))).unwrap();
        assert_eq!(ids(&store), ["b", "a"]);
        assert_eq!(positions(&store), [("a".into(), 2), ("b".into(), 1)]);
    }

    #[test]
    fn puts_a_repository_listed_twice_where_it_is_last_listed() {
        let store = store_with(&["a", "b", "c"]);
        smol::block_on(store.reorder(&ids_of(&["a", "b", "a"]))).unwrap();
        assert_eq!(
            positions(&store),
            [("a".into(), 2), ("b".into(), 1), ("c".into(), 5)]
        );
        assert_eq!(ids(&store), ["b", "a", "c"]);
    }

    #[test]
    fn adds_after_reordered_repositories() {
        let store = store_with(&["a", "b", "c"]);
        smol::block_on(store.reorder(&ids_of(&["b"]))).unwrap();
        smol::block_on(store.create(&repository("d"))).unwrap();
        assert_eq!(ids(&store), ["b", "a", "c", "d"]);
    }

    #[test]
    fn reorders_all_or_nothing() {
        let store = store_with(&["a", "b"]);
        // A trigger that fails the second update, as a full disk might.
        let a = id("a");
        smol::block_on(store.db.run(move |conn| {
            conn.execute_batch(&format!(
                r#"CREATE TRIGGER fail BEFORE UPDATE OF "position" ON "repositories"
                   WHEN NEW."id" = '{a}' AND NEW."position" = 1
                   BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;"#,
            ))
        }))
        .unwrap();

        let error = smol::block_on(store.reorder(&ids_of(&["b", "a"]))).unwrap_err();

        assert_eq!(error.to_string(), "database or disk is full");
        assert_eq!(positions(&store), [("a".into(), 0), ("b".into(), 1)]);
    }
}
