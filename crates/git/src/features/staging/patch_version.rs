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
    /// The object names on its header's `index` line, as written: the index's side and the working
    /// tree's, or the commit's and the index's.
    objects: Option<[ObjectName; 2]>,
}

impl PatchVersion {
    /// Names `patch`, a file's patch as the UI got it, by its length, a hash of all of it, and the
    /// object names of both sides on its header's `index` line (git hashes the working tree's side
    /// for it), so one of another index is never named the same: a few milliseconds for a patch of
    /// megabytes.
    pub fn of(patch: &str) -> PatchVersion {
        let hash = patch.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        PatchVersion {
            length: patch.len(),
            hash,
            objects: index_objects(patch),
        }
    }
}

/// `<length>:<hash>`, the hash in hex, then `:<object>..<object>` if it has an `index` line, e.g.
/// to key a cache by.
impl fmt::Display for PatchVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{:016x}", self.length, self.hash)?;
        if let Some([old, new]) = &self.objects {
            write!(f, ":{}..{}", old.as_str(), new.as_str())?;
        }
        Ok(())
    }
}

/// An object name as a patch's `index` line writes it: up to 64 hex digits, as SHA-256's full one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ObjectName {
    length: u8,
    digits: [u8; 64],
}

impl ObjectName {
    /// `name` if it's one: hex digits, as many as a full SHA-256 name at most.
    fn parse(name: &str) -> Option<ObjectName> {
        if name.is_empty() || name.len() > 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut digits = [0; 64];
        digits[..name.len()].copy_from_slice(name.as_bytes());
        Some(ObjectName {
            length: name.len() as u8,
            digits,
        })
    }

    fn as_str(&self) -> &str {
        // Only hex digits are kept.
        std::str::from_utf8(&self.digits[..usize::from(self.length)]).unwrap_or_default()
    }
}

/// The object names on the `index` line of `patch`'s header, up to its first hunk: `index
/// <object>..<object>`, with the file's mode after it if that's the same on both sides. `None` if
/// it has none, as for a change of only the mode.
fn index_objects(patch: &str) -> Option<[ObjectName; 2]> {
    let line = patch
        .split('\n')
        .take_while(|line| !line.starts_with("@@ "))
        .find_map(|line| line.strip_prefix("index "))?;
    let names = line.split(' ').next()?;
    let (old, new) = names.split_once("..")?;
    Some([ObjectName::parse(old)?, ObjectName::parse(new)?])
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
    fn names_a_patch_by_the_object_names_on_its_index_line_too() {
        let patch = format!("{HEADER}\n@@ -1 +1 @@\n-a\n+b\n");
        let version = PatchVersion::of(&patch);
        assert_eq!(
            version
                .objects
                .map(|names| names.map(|name| name.as_str().to_owned())),
            Some(["1".repeat(40), "2".repeat(40)])
        );
        // Of another index, with the same length and hash, as two patches could be.
        let other = PatchVersion {
            objects: index_objects(&patch.replacen("1111", "3333", 1)),
            ..version
        };
        assert_ne!(version, other);
    }

    #[test]
    fn reads_the_object_names_on_the_index_line_before_the_first_hunk_only() {
        let names = |patch: &str| {
            index_objects(patch).map(|names| names.map(|name| name.as_str().to_owned()))
        };
        let sha256 = "a".repeat(64);
        assert_eq!(
            names(&format!(
                "diff --git a/f b/f\nnew file mode 100644\nindex 0000..{sha256}\n--- /dev/null"
            )),
            Some(["0000".to_owned(), sha256])
        );
        assert_eq!(
            names("diff --git a/f b/f\nold mode 100644\nnew mode 100755\n"),
            None
        );
        assert_eq!(
            names("diff --git a/f b/f\n@@ -1 +1 @@\nindex 1111..2222\n"),
            None
        );
        assert_eq!(names("index 1111..not-hex\n"), None);
        assert_eq!(names("the patch"), None);
    }

    #[test]
    fn names_a_patch_without_hunks_by_all_of_it() {
        assert_ne!(
            PatchVersion::of("Binary files differ\n"),
            PatchVersion::of("Binary files diffir\n")
        );
    }

    #[test]
    fn is_written_as_its_length_hash_and_object_names() {
        // FNV-1a's offset basis, for nothing hashed.
        assert_eq!(PatchVersion::of("").to_string(), "0:cbf29ce484222325");
        assert_eq!(PatchVersion::of("a").to_string(), "1:af63dc4c8601ec8c");
        assert_eq!(
            PatchVersion::of("é").to_string().split(':').next(),
            Some("2")
        );
        assert!(
            PatchVersion::of("index 1111..2222 100644\n")
                .to_string()
                .ends_with(":1111..2222")
        );
    }
}
