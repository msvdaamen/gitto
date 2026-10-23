//! Names a file's patch by what's in it, so the UI can say which patch it picked lines from without
//! sending it back (see `stage_lines`), and name it in its caches. Ported from `patch-version.ts`,
//! which the renderer shared: the name is the one it made, counted in UTF-16 code units as
//! JavaScript counts a string's characters.

/// Names `patch` by what's in it: its length, a hash of its text, and its header, which has the
/// object names of both sides on its `index` line (git hashes the working tree's side for it).
/// Another patch of the same file, as after a save, gets another name.
pub fn patch_version(patch: &str) -> String {
    let length = patch.encode_utf16().count();
    format!("{length}:{}:{}", hash(patch), patch_header(patch))
}

/// `patch` up to its first hunk: the whole of it when it has none.
fn patch_header(patch: &str) -> &str {
    match patch.find("\n@@ ") {
        Some(body) => &patch[..body],
        None => patch,
    }
}

/// FNV-1a over `text`'s characters (UTF-16 code units, as in JavaScript), in base 36: a few
/// milliseconds for a patch of megabytes.
fn hash(text: &str) -> String {
    let mut value: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        value = (value ^ u32::from(unit)).wrapping_mul(0x0100_0193);
    }
    base36(value)
}

/// `value` in base 36, as JavaScript's `toString(36)` writes it.
fn base36(mut value: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = Vec::new();
    loop {
        digits.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).expect("ASCII digits")
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
        assert_eq!(patch_version(&patch), patch_version(&patch.clone()));
        assert_ne!(
            patch_version(&patch),
            patch_version(&patch.replacen("2222", "3333", 1))
        );
        assert_ne!(
            patch_version(&patch),
            patch_version(&format!("{patch}+c\n"))
        );
        // The same length and header, other lines.
        assert_ne!(
            patch_version(&patch),
            patch_version(&format!("{HEADER}\n@@ -1 +1 @@\n-a\n+c\n"))
        );
    }

    #[test]
    fn names_a_patch_without_hunks_by_all_of_it() {
        assert_ne!(
            patch_version("Binary files differ\n"),
            patch_version("Binary files diffir\n")
        );
    }

    #[test]
    fn names_a_patch_as_javascript_does() {
        // As `patchVersion` names them in Node: counted and hashed in UTF-16 code units.
        assert_eq!(patch_version(""), "0:ztntfp:");
        assert_eq!(patch_version("a"), "1:1r9wi7g:a");
        assert_eq!(patch_version("é😀\n@@ x"), "8:160e0e1:é😀");
        assert_eq!(base36(0), "0");
        assert_eq!(base36(u32::MAX), "1z141z3");
    }
}
