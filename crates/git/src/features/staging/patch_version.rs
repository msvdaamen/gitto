//! Names a file's patch by what's in it, so the UI can say which patch it picked lines from without
//! sending it back (see `stage_lines`), and name it in its caches.

use std::fmt;

/// A patch's name, made from what's in it (see `PatchVersion::of`): another patch of the same file,
/// as after a save, gets another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PatchVersion {
    /// Its length, in bytes.
    length: usize,
    /// FNV-1a over its bytes.
    hash: u64,
}

impl PatchVersion {
    /// Names `patch`, a file's patch as the UI got it, by its length and a hash of all of it,
    /// header included, which has the object names of both sides on its `index` line (git hashes
    /// the working tree's side for it): a few milliseconds for a patch of megabytes.
    pub fn of(patch: &str) -> PatchVersion {
        let hash = patch.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        PatchVersion {
            length: patch.len(),
            hash,
        }
    }
}

/// `<length>:<hash>`, the hash in hex, e.g. to key a cache by.
impl fmt::Display for PatchVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{:016x}", self.length, self.hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "diff --git a/f.txt b/f.txt\n\
        index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644\n\
        --- a/f.txt\n\
        +++ b/f.txt";

    #[test]
    fn names_the_same_patch_the_same_and_another_patch_of_the_file_otherwise() {
        let patch = format!("{HEADER}\n@@ -1 +1 @@\n-a\n+b\n");
        assert_eq!(PatchVersion::of(&patch), PatchVersion::of(&patch.clone()));
        assert_ne!(
            PatchVersion::of(&patch),
            PatchVersion::of(&patch.replacen("2222", "3333", 1))
        );
        assert_ne!(
            PatchVersion::of(&patch),
            PatchVersion::of(&format!("{patch}+c\n"))
        );
        // The same length and header, other lines.
        assert_ne!(
            PatchVersion::of(&patch),
            PatchVersion::of(&format!("{HEADER}\n@@ -1 +1 @@\n-a\n+c\n"))
        );
    }

    #[test]
    fn names_a_patch_without_hunks_by_all_of_it() {
        assert_ne!(
            PatchVersion::of("Binary files differ\n"),
            PatchVersion::of("Binary files diffir\n")
        );
    }

    #[test]
    fn is_written_as_its_length_and_hash() {
        // FNV-1a's offset basis, for nothing hashed.
        assert_eq!(PatchVersion::of("").to_string(), "0:cbf29ce484222325");
        assert_eq!(PatchVersion::of("a").to_string(), "1:af63dc4c8601ec8c");
        assert_eq!(
            PatchVersion::of("é").to_string().split(':').next(),
            Some("2")
        );
    }
}
