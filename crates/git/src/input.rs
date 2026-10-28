//! Input the features take from the UI, checked when it's made. Ported from
//! `packages/git/src/input.ts`, and the checks of each feature's `contract.ts`: their zod schemas
//! are the newtypes here, so a feature can't be called with input the contract refused. Each is
//! made with `parse` (or `TryFrom<String>`, which serde uses), fails with `Error::InvalidInput`
//! saying why, and derefs to what it holds (a string's pass as git arguments, `AsRef<OsStr>`).
//!
//! TS's `RepositoryInput` (a procedure's `repositoryId`) is [`RepositoryId`]: the features take
//! the `Repo` that `GitApi::open` opens with it instead (see `crate::api`).
//!
//! The checks of a feature's own input are its own types: a page of the log is
//! `history::commands::Page`, lines of a patch `staging::schema::LineRange`.

use std::borrow::Borrow;
use std::ffi::OsStr;
use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::core::errors::{Error, Result};

macro_rules! checked_string {
    ($(#[$doc:meta])* $name:ident, $valid:expr, $message:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
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

        // It hashes and compares as the string it holds, so a map keyed by it is looked up by
        // a `&str`.
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
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

checked_string!(
    /// A repository's id in Gitto: a UUIDv7, hyphenated.
    RepositoryId,
    is_uuid_v7,
    |value| format!("Not a repository id. ({value:?})")
);

checked_string!(
    /// A local or remote branch, by its full ref name (`refs/heads/…` or `refs/remotes/…`).
    BranchRef,
    |value| {
        ["refs/heads/", "refs/remotes/"].iter().any(|prefix| {
            value
                .strip_prefix(prefix)
                .and_then(|name| name.chars().next())
                // zod's `/^refs\/(heads|remotes)\/./`: `.` is anything but a line terminator.
                .is_some_and(|first| !matches!(first, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        })
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
    /// A string that isn't empty (zod's `z.string().min(1)`), e.g. a branch's short name the UI
    /// saw checked out, or a conflicted file's path.
    NonEmpty,
    |value| !value.is_empty(),
    |_| "Too small: expected string to have >=1 characters".to_owned()
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct Paths(Vec<String>);

impl Paths {
    /// `paths`, if there's one and none is empty; `Error::InvalidInput` otherwise.
    pub fn parse<I, S>(paths: I) -> Result<Paths>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let paths: Vec<String> = paths.into_iter().map(Into::into).collect();
        if paths.is_empty() {
            return Err(Error::InvalidInput("No paths.".to_owned()));
        }
        if paths.iter().any(String::is_empty) {
            return Err(Error::InvalidInput("Not a path.".to_owned()));
        }
        Ok(Paths(paths))
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl TryFrom<Vec<String>> for Paths {
    type Error = Error;

    fn try_from(paths: Vec<String>) -> Result<Paths> {
        Paths::parse(paths)
    }
}

impl From<Paths> for Vec<String> {
    fn from(paths: Paths) -> Vec<String> {
        paths.0
    }
}

impl Deref for Paths {
    type Target = [String];

    fn deref(&self) -> &[String] {
        &self.0
    }
}

/// Whether `value` has a character JS's `\S` matches: one that isn't whitespace as JS has it,
/// which is Unicode's but for U+0085 (NEL), and with the byte order mark.
fn has_non_space(value: &str) -> bool {
    value
        .chars()
        .any(|c| !(c.is_whitespace() && c != '\u{85}') && c != '\u{feff}')
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// zod's `uuidv7`: `xxxxxxxx-xxxx-7xxx-[89ab]xxx-xxxxxxxxxxxx`, in either case.
fn is_uuid_v7(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            14 => *b == b'7',
            19 => matches!(b.to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b'),
            _ => b.is_ascii_hexdigit(),
        })
}

/// A file, by its path in the repository and, for a rename, its previous one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInput {
    pub path: String,
    pub orig_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(error: Error) -> String {
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
        assert_eq!(error.api().code(), "BAD_REQUEST");
        error.to_string()
    }

    #[test]
    fn a_map_keyed_by_one_is_looked_up_by_its_string() {
        let id = "01890a5d-ac96-774b-bcce-b302099a8057";
        let map = std::collections::HashMap::from([(RepositoryId::parse(id).unwrap(), 1)]);
        assert_eq!(map.get(id), Some(&1));
        assert_eq!(map.get("01890a5d-ac96-774b-bcce-b302099a8058"), None);
    }

    #[test]
    fn shas_are_lowercase_hex() {
        assert!(Sha::parse("abcd").is_ok());
        assert!(Sha::parse("a".repeat(64)).is_ok());
        assert!(Sha::parse("abc").is_err());
        assert!(Sha::parse("a".repeat(65)).is_err());
        assert!(Sha::parse("ABCD").is_err());
        assert!(Sha::parse("HEAD").is_err());
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
        assert!(BranchRef::parse("refs/heads/").is_err());
        assert!(BranchRef::parse("refs/heads/\nmain").is_err());
        assert!(BranchRef::parse("main").is_err());
        assert_eq!(
            invalid(BranchRef::parse("refs/tags/v1").unwrap_err()),
            "Not a branch. (\"refs/tags/v1\")"
        );
    }

    #[test]
    fn repository_ids_are_uuid_v7() {
        assert!(RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").is_ok());
        assert!(RepositoryId::parse("01890A5D-AC96-774B-BCCE-B302099A8057").is_ok());
        assert!(RepositoryId::parse(uuid::Uuid::now_v7().to_string()).is_ok());
        // A UUIDv4, and one with the wrong variant.
        assert!(RepositoryId::parse("01890a5d-ac96-474b-bcce-b302099a8057").is_err());
        assert!(RepositoryId::parse("01890a5d-ac96-774b-ccce-b302099a8057").is_err());
        assert!(RepositoryId::parse("missing").is_err());
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
        // NEL isn't whitespace to JS's `\S`.
        assert!(Message::parse("\u{85}").is_ok());
    }

    #[test]
    fn non_empty_strings_may_be_whitespace() {
        assert!(NonEmpty::parse(" ").is_ok());
        assert_eq!(
            invalid(NonEmpty::parse("").unwrap_err()),
            "Too small: expected string to have >=1 characters"
        );
    }

    #[test]
    fn paths_are_one_or_more_and_none_empty() {
        let paths = Paths::parse(["a", "b c"]).unwrap();
        assert_eq!(&*paths, ["a", "b c"]);
        assert_eq!(
            invalid(Paths::parse(Vec::<String>::new()).unwrap_err()),
            "No paths."
        );
        assert_eq!(invalid(Paths::parse(["a", ""]).unwrap_err()), "Not a path.");
    }

    #[test]
    fn input_is_checked_when_its_deserialized() {
        let sha: Result<Sha, _> = serde_json::from_str("\"zzzz\"");
        assert!(sha.is_err());
        let paths: Result<Paths, _> = serde_json::from_str("[]");
        assert!(paths.is_err());
        let name: Result<BranchName, _> = serde_json::from_str("\" \"");
        assert!(name.is_err());
        let id: RepositoryId =
            serde_json::from_str("\"01890a5d-ac96-774b-bcce-b302099a8057\"").unwrap();
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            "\"01890a5d-ac96-774b-bcce-b302099a8057\""
        );
        let file: FileInput = serde_json::from_str(r#"{"path":"a","origPath":null}"#).unwrap();
        assert_eq!(file.path, "a");
    }
}
