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

/// Where the id is `?`, which is in lower case, as ids are written: the row's is compared in lower
/// case too, so one put in by hand in upper case is found as well.
const ID_IS: &str = r#"lower("repositories"."id") = ?"#;

/// Where the path is `?`: exactly that path.
const PATH_IS: &str = r#""repositories"."path" = ?"#;

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

    /// Every repository, in the order of their tabs. A row that's no repository, with an id that
    /// isn't a repository id say, is left out with a warning, rather than failing the list.
    pub async fn get_all(&self) -> rusqlite::Result<Vec<Repository>> {
        self.db.run(|conn| get_all(conn)).await
    }

    pub async fn get_by_id(&self, id: &RepositoryId) -> rusqlite::Result<Option<Repository>> {
        let id = id.to_string();
        self.db.run(move |conn| get_by(conn, ID_IS, &id)).await
    }

    pub async fn get_by_path(&self, path: &str) -> rusqlite::Result<Option<Repository>> {
        let path = path.to_owned();
        self.db.run(move |conn| get_by(conn, PATH_IS, &path)).await
    }

    /// Adds `repository` after the others, unless one with its path is listed already: resolves to
    /// the one listed at its path, which is that one if it was. So adding a folder twice at once,
    /// with a double click say, lists it once, and neither fails.
    pub async fn create(&self, repository: &Repository) -> rusqlite::Result<Repository> {
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
/// one put in by hand may not be; one that isn't a repository id fails, as it's no row of Gitto's.
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
    let mut repositories = Vec::new();
    for row in conn
        .prepare(&format!(
            r#"select {COLUMNS} from "repositories" order by "repositories"."position" asc"#
        ))?
        .query_map([], repository)?
    {
        match row {
            Ok(repository) => repositories.push(repository),
            Err(
                error @ (rusqlite::Error::FromSqlConversionFailure(..)
                | rusqlite::Error::InvalidColumnType(..)),
            ) => {
                log::warn!("the list of repositories left out a row that's no repository: {error}")
            }
            Err(error) => return Err(error),
        }
    }
    Ok(repositories)
}

/// The repository where `condition` holds for `value` ([`ID_IS`] or [`PATH_IS`]), if there's one.
/// `condition` goes into the SQL as it is, so it's only ever a literal.
fn get_by(
    conn: &Connection,
    condition: &'static str,
    value: &str,
) -> rusqlite::Result<Option<Repository>> {
    conn.query_row(
        &format!(r#"select {COLUMNS} from "repositories" where {condition}"#),
        [value],
        repository,
    )
    .optional()
}

fn create(conn: &Connection, repository: &Repository) -> rusqlite::Result<Repository> {
    conn.execute(
        r#"insert into "repositories" ("id", "name", "path", "position") values (?, ?, ?, (SELECT coalesce(max("repositories"."position"), -1) + 1 FROM "repositories")) on conflict ("path") do nothing"#,
        params![repository.id.to_string(), repository.name, repository.path],
    )?;
    // There is one now, whichever add put it there.
    get_by(conn, PATH_IS, &repository.path)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn delete(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        &format!(r#"delete from "repositories" where {ID_IS}"#),
        [id],
    )?;
    Ok(())
}

/// `index` as a position, SQLite's integer: one that doesn't fit, which no list's can, fails.
fn position(index: usize) -> rusqlite::Result<i64> {
    i64::try_from(index).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn reorder(conn: &mut Connection, ids: &[String]) -> rusqlite::Result<()> {
    // Past the end of `ids`, so any repository missing from it lands after the ones in it.
    let rest = position(ids.len())?;
    // `begin`, as Drizzle: deferred.
    let tx = conn.transaction()?;
    tx.execute(
        r#"update "repositories" set "position" = "repositories"."position" + ?"#,
        [rest],
    )?;
    let update = format!(r#"update "repositories" set "position" = ? where {ID_IS}"#);
    for (index, id) in ids.iter().enumerate() {
        tx.execute(&update, params![position(index)?, id])?;
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

    /// Puts a row in by hand, as `values` of its `id`, `name`, `path` and `position`.
    fn insert(store: &RepositoryStore, values: &'static str) {
        smol::block_on(store.db.run(move |conn| {
            conn.execute_batch(&format!(
                r#"insert into "repositories" ("id", "name", "path", "position") values {values}"#
            ))
        }))
        .unwrap();
    }

    #[test]
    fn reads_finds_removes_and_moves_a_repository_whose_id_is_in_upper_case() {
        let store = store_with(&["a"]);
        insert(
            &store,
            "('01900000-0000-7000-8000-00000000000B', 'b', '/repos/b', 1)",
        );
        let b = RepositoryId::parse("01900000-0000-7000-8000-00000000000b").unwrap();
        let all = smol::block_on(store.get_all()).unwrap();
        assert_eq!(all[1].id, b);
        assert_eq!(
            all[1].id.to_string(),
            "01900000-0000-7000-8000-00000000000b"
        );
        assert_eq!(
            smol::block_on(store.get_by_id(&b)).unwrap(),
            Some(all[1].clone())
        );

        smol::block_on(store.reorder(&[b.clone(), id("a")])).unwrap();
        let order: Vec<RepositoryId> = smol::block_on(store.get_all())
            .unwrap()
            .into_iter()
            .map(|repository| repository.id)
            .collect();
        assert_eq!(order, [b.clone(), id("a")]);

        smol::block_on(store.delete(&b)).unwrap();
        assert_eq!(ids(&store), ["a"]);
    }

    #[test]
    fn leaves_out_rows_that_arent_repositories() {
        let store = store_with(&["a"]);
        insert(&store, "('b', 'b', '/repos/b', 1)");
        insert(&store, "(NULL, 'c', '/repos/c', 2)");
        smol::block_on(store.create(&repository("d"))).unwrap();
        assert_eq!(ids(&store), ["a", "d"]);
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
    fn keeps_the_repository_already_at_a_path() {
        let store = store_with(&["a"]);
        let same_path = Repository {
            id: id("other"),
            ..repository("a")
        };
        assert_eq!(
            smol::block_on(store.create(&same_path)).unwrap(),
            repository("a")
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
