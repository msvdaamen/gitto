//! Input the features take from the UI, checked when it's made. Ported from
//! `packages/git/src/input.ts`, whose zod schemas are the newtypes here: each is made with `parse`
//! (or `TryFrom<String>`, which serde uses), fails with `Error::InvalidInput`, and derefs to `str`
//! and passes as a git argument (`AsRef<OsStr>`).
//!
//! TS's `RepositoryInput` (a procedure's `repositoryId`) is [`RepositoryId`]: the features take
//! the `Repo` it opens (`GitRepos::open`) instead.

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
                if valid(&value) {
                    Ok($name(value))
                } else {
                    Err(Error::InvalidInput(format!("{} ({value:?})", $message)))
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

checked_string!(
    /// A repository's id in Gitto: a UUIDv7, hyphenated.
    RepositoryId,
    is_uuid_v7,
    "Not a repository id."
);

checked_string!(
    /// A local or remote branch, by its full ref name (`refs/heads/…` or `refs/remotes/…`).
    BranchRef,
    |value| {
        ["refs/heads/", "refs/remotes/"]
            .iter()
            .any(|prefix| value.strip_prefix(prefix).is_some_and(|name| !name.is_empty()))
    },
    "Not a branch."
);

checked_string!(
    /// A full or abbreviated commit SHA: 4 to 64 lowercase hex digits.
    Sha,
    |value| (4..=64).contains(&value.len()) && is_lower_hex(value),
    "Not a commit SHA."
);

checked_string!(
    /// A full commit SHA: SHA-1 (40 hex digits), or SHA-256 (64).
    FullSha,
    |value| matches!(value.len(), 40 | 64) && is_lower_hex(value),
    "Not a full commit SHA."
);

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

    #[test]
    fn shas_are_lowercase_hex() {
        assert!(Sha::parse("abcd").is_ok());
        assert!(Sha::parse("abc").is_err());
        assert!(Sha::parse("ABCD").is_err());
        assert!(FullSha::parse("a".repeat(40)).is_ok());
        assert!(FullSha::parse("a".repeat(64)).is_ok());
        assert!(FullSha::parse("a".repeat(41)).is_err());
    }

    #[test]
    fn branch_refs_are_local_or_remote_branches() {
        assert!(BranchRef::parse("refs/heads/main").is_ok());
        assert!(BranchRef::parse("refs/remotes/origin/main").is_ok());
        assert!(BranchRef::parse("refs/heads/").is_err());
        assert_eq!(
            BranchRef::parse("refs/tags/v1").unwrap_err().to_string(),
            "Not a branch. (\"refs/tags/v1\")"
        );
    }

    #[test]
    fn repository_ids_are_uuid_v7() {
        assert!(RepositoryId::parse("01890a5d-ac96-774b-bcce-b302099a8057").is_ok());
        assert!(RepositoryId::parse("01890A5D-AC96-774B-BCCE-B302099A8057").is_ok());
        assert!(RepositoryId::parse("01890a5d-ac96-474b-bcce-b302099a8057").is_err());
        assert!(RepositoryId::parse("missing").is_err());
    }

    #[test]
    fn input_is_checked_when_its_deserialized() {
        let sha: Result<Sha, _> = serde_json::from_str("\"zzzz\"");
        assert!(sha.is_err());
        let file: FileInput = serde_json::from_str(r#"{"path":"a","origPath":null}"#).unwrap();
        assert_eq!(file.path, "a");
    }
}
