//! Input the features take from the UI, checked when it's made. Ported from
//! `packages/git/src/input.ts`, and the checks of each feature's `contract.ts`: their zod schemas
//! are the newtypes here, so a feature can't be called with input the contract refused. Each is
//! made with `parse` (or `TryFrom<String>`), fails with `Error::InvalidInput` saying why, and
//! derefs to what it holds (a string's pass as git arguments, `AsRef<OsStr>`).
//!
//! TS's `RepositoryInput` (a procedure's `repositoryId`) is [`RepositoryId`]: the features take
//! the `Repo` that `GitApi::call` opens with it instead (see `crate::api`).
//!
//! The checks of a feature's own input are its own types: a page of the log is
//! `history::commands::Page`, lines of a patch `staging::schema::LineRange`.

use std::ffi::OsStr;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

use uuid::Uuid;

use crate::core::errors::{Error, Result};

macro_rules! checked_string {
    ($(#[$doc:meta])* $name:ident, $valid:expr, $message:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            /// `value`, if it's valid; `Error::InvalidInput` otherwise.
            pub fn parse(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                let valid: fn(&str) -> bool = $valid;
                let message: fn(&str) -> String = $message;
                if valid(&value) {
                    Ok($name(value))
                } else {
                    Err(Error::InvalidInput(message(&value)))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;

            fn try_from(value: String) -> Result<Self> {
                Self::parse(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = Error;

            fn try_from(value: &str) -> Result<Self> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<OsStr> for $name {
            fn as_ref(&self) -> &OsStr {
                OsStr::new(&self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

/// A repository's id in Gitto: a UUIDv7, written hyphenated. Read in either case, and shown in
/// lower case, so an id is the same `RepositoryId` however it was written.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepositoryId(Uuid);

impl RepositoryId {
    /// A new id, for a repository being added to Gitto.
    pub fn generate() -> Self {
        RepositoryId(Uuid::now_v7())
    }

    /// `value`, if it's a hyphenated UUIDv7; `Error::InvalidInput` otherwise.
    pub fn parse(value: impl AsRef<str>) -> Result<Self> {
        let value = value.as_ref();
        // Only the hyphenated form: `Uuid` also reads the simple, braced and URN ones.
        Uuid::try_parse(value)
            .ok()
            .filter(|uuid| {
                value.len() == 36
                    && uuid.get_version_num() == 7
                    && uuid.get_variant() == uuid::Variant::RFC4122
            })
            .map(RepositoryId)
            .ok_or_else(|| Error::InvalidInput(format!("Not a repository id. ({value:?})")))
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl FromStr for RepositoryId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

impl TryFrom<String> for RepositoryId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for RepositoryId {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

/// Hyphenated, in lower case.
impl fmt::Display for RepositoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.hyphenated(), f)
    }
}

checked_string!(
    /// A local or remote branch, by its full ref name (`refs/heads/…` or `refs/remotes/…`). Not
    /// a remote's `refs/remotes/<remote>/HEAD`, a link to its default branch rather than a branch.
    BranchRef,
    |value| {
        let named = ["refs/heads/", "refs/remotes/"].iter().any(|prefix| {
            value
                .strip_prefix(prefix)
                .and_then(|name| name.chars().next())
                // zod's `/^refs\/(heads|remotes)\/./`: `.` is anything but a line terminator.
                .is_some_and(|first| !matches!(first, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        });
        named && !(value.starts_with("refs/remotes/") && value.ends_with("/HEAD"))
    },
    |value| format!("Not a branch. ({value:?})")
);

checked_string!(
    /// A full or abbreviated commit SHA: 4 to 64 lowercase hex digits.
    Sha,
    |value| (4..=64).contains(&value.len()) && is_lower_hex(value),
    |value| format!("Not a commit SHA. ({value:?})")
);

checked_string!(
    /// A full commit SHA: SHA-1 (40 hex digits), or SHA-256 (64).
    FullSha,
    |value| matches!(value.len(), 40 | 64) && is_lower_hex(value),
    |value| format!("Not a full commit SHA. ({value:?})")
);

/// A full SHA is a SHA too.
impl From<FullSha> for Sha {
    fn from(sha: FullSha) -> Sha {
        Sha(sha.0)
    }
}

checked_string!(
    /// A file's path in the repository, as git names it: not empty, e.g. a conflicted file's.
    RepoPath,
    |value| !value.is_empty(),
    |_| "The path is empty.".to_owned()
);

checked_string!(
    /// A local branch, by its short name: not empty, e.g. the one the UI saw checked out.
    BranchShortName,
    |value| !value.is_empty(),
    |_| "The branch name is empty.".to_owned()
);

checked_string!(
    /// A name for a new branch: anything but only whitespace (zod's `.regex(/\S/)`). Git says
    /// whether it's a valid branch name.
    BranchName,
    has_non_space,
    |_| "The name is empty.".to_owned()
);

checked_string!(
    /// A folder to make a worktree in: anything but only whitespace.
    Folder,
    has_non_space,
    |_| "The folder is empty.".to_owned()
);

checked_string!(
    /// A commit's message: anything but only whitespace. Not trimmed: an amend, or a reword,
    /// commits it exactly as it's sent, ending in a newline or not.
    Message,
    has_non_space,
    |_| "The message is empty.".to_owned()
);

/// Paths of files in the repository: at least one, none of them empty (the staging contract's
/// `z.array(z.string().min(1)).min(1)`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Paths(Vec<RepoPath>);

impl Paths {
    /// `paths`, if there's one and none is empty; `Error::InvalidInput` otherwise.
    pub fn parse<I, S>(paths: I) -> Result<Paths>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let paths = paths
            .into_iter()
            .map(RepoPath::parse)
            .collect::<Result<Vec<_>>>()?;
        if paths.is_empty() {
            return Err(Error::InvalidInput("No paths.".to_owned()));
        }
        Ok(Paths(paths))
    }

    pub fn as_slice(&self) -> &[RepoPath] {
        &self.0
    }
}

impl TryFrom<Vec<String>> for Paths {
    type Error = Error;

    fn try_from(paths: Vec<String>) -> Result<Paths> {
        Paths::parse(paths)
    }
}

impl From<Paths> for Vec<RepoPath> {
    fn from(paths: Paths) -> Vec<RepoPath> {
        paths.0
    }
}

impl Deref for Paths {
    type Target = [RepoPath];

    fn deref(&self) -> &[RepoPath] {
        &self.0
    }
}

/// Whether `value` has a character JS's `\S` matches: one that isn't whitespace as JS's `\s` has
/// it, which is Unicode's but for the next line control (U+0085), and has the byte order mark.
fn has_non_space(value: &str) -> bool {
    value.chars().any(|c| match c {
        '\u{85}' => true,
        '\u{feff}' => false,
        c => !c.is_whitespace(),
    })
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A file, by its path in the repository and, for a rename, its previous one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileInput {
    pub path: RepoPath,
    pub orig_path: Option<RepoPath>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::valid;

    fn invalid(error: Error) -> String {
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
        assert_eq!(error.api().code(), "BAD_REQUEST");
        error.to_string()
    }

    #[test]
    fn shas_are_lowercase_hex() {
        assert!(Sha::parse("abcd").is_ok());
        assert!(Sha::parse("a".repeat(64)).is_ok());
        assert!(Sha::parse("abc").is_err());
        assert!(Sha::parse("a".repeat(65)).is_err());
        assert!(Sha::parse("ABCD").is_err());
        assert!(Sha::parse("HEAD").is_err());
        // Never read by git as an option.
        assert!(Sha::parse("--output=/tmp/x").is_err());
        assert!(FullSha::parse("a".repeat(40)).is_ok());
        assert!(FullSha::parse("a".repeat(64)).is_ok());
        assert!(FullSha::parse("a".repeat(41)).is_err());
        assert!(FullSha::parse("abcd").is_err());
        assert_eq!(
            invalid(FullSha::parse("abcd").unwrap_err()),
            "Not a full commit SHA. (\"abcd\")"
        );
    }

    #[test]
    fn branch_refs_are_local_or_remote_branches() {
        assert!(BranchRef::parse("refs/heads/main").is_ok());
        assert!(BranchRef::parse("refs/remotes/origin/main").is_ok());
        assert!(BranchRef::parse("refs/remotes/up/stream/main").is_ok());
        // A remote's link to its default branch, whatever the remote's name.
        assert_eq!(
            invalid(BranchRef::parse("refs/remotes/origin/HEAD").unwrap_err()),
            "Not a branch. (\"refs/remotes/origin/HEAD\")"
        );
        assert!(BranchRef::parse("refs/remotes/up/stream/HEAD").is_err());
        // A local branch can't be named `HEAD`, but one under a folder of that name is one.
        assert!(BranchRef::parse("refs/heads/feature/HEAD").is_ok());
        assert!(BranchRef::parse("refs/heads/").is_err());
        assert!(BranchRef::parse("refs/heads/\nmain").is_err());
        assert!(BranchRef::parse("main").is_err());
        assert_eq!(
            invalid(BranchRef::parse("refs/tags/v1").unwrap_err()),
            "Not a branch. (\"refs/tags/v1\")"
        );
    }

    #[test]
    fn messages_have_a_character_that_isnt_whitespace_as_js_has_it() {
        assert!(Message::parse("Fix").is_ok());
        assert!(Message::parse(" \t\n\r\u{a0}\u{2028}\u{3000}").is_err());
        assert!(Message::parse("\u{feff}").is_err());
        assert!(Message::parse("\u{85}").is_ok());
    }

    #[test]
    fn repository_ids_are_uuid_v7() {
        assert!(RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").is_ok());
        assert!(RepositoryId::parse(uuid::Uuid::now_v7().to_string()).is_ok());
        let generated = RepositoryId::generate();
        assert_eq!(
            RepositoryId::parse(generated.to_string()).unwrap(),
            generated
        );
        // A UUIDv4, and one with the wrong variant.
        assert!(RepositoryId::parse("01890a5d-ac96-474b-bcce-b302099a8057").is_err());
        assert!(RepositoryId::parse("01890a5d-ac96-774b-ccce-b302099a8057").is_err());
        // Only hyphenated.
        assert!(RepositoryId::parse("01890a5dac96774bbcceb302099a8057").is_err());
        assert!(RepositoryId::parse("{01890a5d-ac96-774b-bcce-b302099a8057}").is_err());
        assert_eq!(
            invalid(RepositoryId::parse("missing").unwrap_err()),
            "Not a repository id. (\"missing\")"
        );
    }

    #[test]
    fn repository_ids_are_the_same_in_either_case() {
        let lower = RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").unwrap();
        let upper = RepositoryId::parse("01890A5D-AC96-774B-BCCE-B302099A8057").unwrap();
        assert_eq!(upper, lower);
        assert_eq!(upper.to_string(), "01890a5d-ac96-774b-bcce-b302099a8057");
    }

    #[test]
    fn names_folders_and_messages_need_more_than_whitespace() {
        assert_eq!(BranchName::parse("topic").unwrap().as_str(), "topic");
        assert_eq!(
            invalid(BranchName::parse(" \t\n").unwrap_err()),
            "The name is empty."
        );
        assert_eq!(
            invalid(Folder::parse("").unwrap_err()),
            "The folder is empty."
        );
        assert_eq!(
            invalid(Message::parse("\u{feff} \u{3000}").unwrap_err()),
            "The message is empty."
        );
        // Kept as it's sent.
        assert_eq!(Message::parse(" Fix\n").unwrap().as_str(), " Fix\n");
        // NEL is whitespace, as Unicode has it.
        assert!(Message::parse("\u{85}").is_err());
    }

    #[test]
    fn paths_and_branch_names_may_be_whitespace_but_not_empty() {
        assert!(RepoPath::parse(" ").is_ok());
        assert_eq!(
            invalid(RepoPath::parse("").unwrap_err()),
            "The path is empty."
        );
        assert!(BranchShortName::parse(" ").is_ok());
        assert_eq!(
            invalid(BranchShortName::parse("").unwrap_err()),
            "The branch name is empty."
        );
    }

    #[test]
    fn paths_are_one_or_more_and_none_empty() {
        let paths = Paths::parse(["a", "b c"]).unwrap();
        assert_eq!(paths.as_slice(), [valid::<RepoPath>("a"), valid("b c")]);
        assert_eq!(
            invalid(Paths::parse(Vec::<String>::new()).unwrap_err()),
            "No paths."
        );
        assert_eq!(
            invalid(Paths::parse(["a", ""]).unwrap_err()),
            "The path is empty."
        );
    }
}
