//! Reads what `git stash list` prints. Ported from `parse.ts`.

use super::schema::Stash;

/// The fields of each stash: `%H`, `%P`, `%ct`, `%gs`.
const FIELDS: usize = 4;

/// How `stash list -z` is read: every field and entry ends in a NUL, which none of them can
/// contain.
pub const STASH_LIST_FORMAT: &str = "--format=%H%x00%P%x00%ct%x00%gs";

/// The stashes in `stash list`'s output (see `STASH_LIST_FORMAT`), newest first.
pub fn parse_stash_list(output: &str) -> Vec<Stash> {
    let fields: Vec<&str> = output.split('\0').collect();
    fields
        .chunks_exact(FIELDS)
        .map(|stash| {
            let [sha, parents, created_at, message] = [stash[0], stash[1], stash[2], stash[3]];
            let base = parents.split(' ').next().unwrap_or("");
            // A time that isn't a number (it always is) is taken as none.
            let seconds = created_at.trim().parse::<i64>().unwrap_or(0);
            Stash {
                sha: sha.to_owned(),
                base: base.to_owned(),
                message: message.to_owned(),
                created_at: seconds * 1000,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_stash_and_its_base() {
        let output = "s1\0b1 i1 u1\x00100\0On main: one\0s2\0b2 i2\x00200\0WIP on main: abc two\0";
        assert_eq!(
            parse_stash_list(output),
            [
                Stash {
                    sha: "s1".into(),
                    base: "b1".into(),
                    message: "On main: one".into(),
                    created_at: 100_000,
                },
                Stash {
                    sha: "s2".into(),
                    base: "b2".into(),
                    message: "WIP on main: abc two".into(),
                    created_at: 200_000,
                },
            ]
        );
        assert_eq!(parse_stash_list(""), []);
    }
}
