//! Ported from `git/patch.ts`: what a file's patch, as git gives it, says about its changes.

use gitto_git::FileMode;
use memchr::memmem;

/// What a file's patch, as git gives it, says about its changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchSummary {
    /// Git can't compare it as text: "Binary files … differ".
    pub binary: bool,
    pub additions: u64,
    pub deletions: u64,
}

/// Where a patch's hunks start: the line break before its first `@@ `.
const HUNKS: &[u8] = b"\n@@ ";

/// Reads `patch`'s lines for what the list of files doesn't always say: an untracked file's, or one
/// of too many to count, has no line counts, and one of those can be binary, or of 100,000 lines.
pub fn summarize_patch(patch: &str) -> PatchSummary {
    let bytes = patch.as_bytes();
    let Some(body) = memmem::find(bytes, HUNKS) else {
        return PatchSummary {
            binary: is_binary(patch),
            additions: 0,
            deletions: 0,
        };
    };
    let mut additions = 0;
    let mut deletions = 0;
    // Each line of the hunks starts with its kind; `\` is "No newline at end of file".
    let mut start = body + 1;
    while start < bytes.len() {
        match bytes[start] {
            b'+' => additions += 1,
            b'-' => deletions += 1,
            b'd' => {
                // Another file's header: a file that changed type is a deletion and an addition to
                // git.
                match memmem::find(&bytes[start..], HUNKS) {
                    Some(hunks) => {
                        start += hunks + 1;
                        continue;
                    }
                    None => break,
                }
            }
            _ => {}
        }
        match memchr::memchr(b'\n', &bytes[start..]) {
            Some(end) => start += end + 1,
            None => break,
        }
    }
    PatchSummary {
        binary: false,
        additions,
        deletions,
    }
}

/// Whether a patch without hunks is of a binary file.
fn is_binary(header: &str) -> bool {
    const BINARY: &str = "Binary files ";
    const DIFFER: &str = " differ";
    header.split('\n').any(|line| {
        line == "GIT binary patch"
            || (line.len() >= BINARY.len() + DIFFER.len()
                && line.starts_with(BINARY)
                && line.ends_with(DIFFER))
    })
}

/// Whether `patch` has lines to stage or unstage left, rather than none, or a mode change.
pub fn has_hunks(patch: &str) -> bool {
    memmem::find(patch.as_bytes(), HUNKS).is_some()
}

/// Whether `patch`'s lines can only be staged all at once: a link's, whose contents are the path it
/// points to, a submodule's, or a file's that changed type, which git has as two files.
pub fn staged_whole(patch: &str) -> bool {
    patch_header(patch).split('\n').any(|line| {
        ends_with_mode(line, FileMode::Link) || ends_with_mode(line, FileMode::Submodule)
    }) || patch.contains("\ndiff --git ")
}

/// Whether `patch` is of a symbolic link, whose contents are the path it points to.
pub fn is_link(patch: &str) -> bool {
    patch_header(patch)
        .split('\n')
        .any(|line| ends_with_mode(line, FileMode::Link))
}

/// Whether the header line `line` ends with the file mode `mode`, as in `new file mode 120000`.
fn ends_with_mode(line: &str, mode: FileMode) -> bool {
    line.strip_suffix(mode.as_str())
        .is_some_and(|rest| rest.ends_with(' '))
}

/// `patch` up to its first hunk: the whole of it when it has none.
fn patch_header(patch: &str) -> &str {
    match memmem::find(patch.as_bytes(), HUNKS) {
        Some(body) => &patch[..body],
        None => patch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "diff --git a/a.txt b/a.txt\n\
        index 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644\n\
        --- a/a.txt\n\
        +++ b/a.txt";

    fn summary(binary: bool, additions: u64, deletions: u64) -> PatchSummary {
        PatchSummary {
            binary,
            additions,
            deletions,
        }
    }

    mod summarize_patch {
        use super::*;

        #[test]
        fn counts_the_lines_added_and_removed_in_the_hunks_not_the_headers() {
            let patch = format!(
                "{HEADER}\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n+C\n c\n\\ No newline at end of file\n"
            );
            assert_eq!(summarize_patch(&patch), summary(false, 2, 1));
        }

        #[test]
        fn tells_a_binary_file_from_one_without_changed_lines() {
            let binary = "diff --git a/x.png b/x.png\nindex 1..2 100644\nBinary files a/x.png and b/x.png differ\n";
            let empty = "diff --git a/e.txt b/e.txt\nnew file mode 100644\nindex 0..e69de29\n";
            assert_eq!(summarize_patch(binary), summary(true, 0, 0));
            assert_eq!(summarize_patch(empty), summary(false, 0, 0));
        }

        #[test]
        fn counts_both_halves_of_a_file_that_changed_type_without_the_second_header() {
            let patch = [
                "diff --git a/l b/l",
                "deleted file mode 120000",
                "index 1111111..0000000",
                "--- a/l",
                "+++ /dev/null",
                "@@ -1 +0,0 @@",
                "-target",
                "\\ No newline at end of file",
                "diff --git a/l b/l",
                "new file mode 100644",
                "index 0000000..2222222",
                "--- /dev/null",
                "+++ b/l",
                "@@ -0,0 +1,2 @@",
                "+one",
                "+two",
                "",
            ]
            .join("\n");
            assert_eq!(summarize_patch(&patch), summary(false, 2, 1));
        }

        #[test]
        fn counts_a_last_line_without_a_newline() {
            assert_eq!(
                summarize_patch(&format!("{HEADER}\n@@ -0,0 +1 @@\n+only")).additions,
                1
            );
        }
    }

    mod has_hunks {
        use super::*;

        #[test]
        fn is_whether_the_patch_has_changed_lines_not_only_a_header() {
            assert!(has_hunks(&format!("{HEADER}\n@@ -1 +1 @@\n-a\n+b\n")));
            assert!(!has_hunks(&format!(
                "{}\nold mode 100644\nnew mode 100755\n",
                HEADER.replacen("100644", "100755", 1)
            )));
        }
    }

    mod staged_whole_and_is_link {
        use super::*;

        fn link() -> String {
            [
                "diff --git a/l b/l",
                "new file mode 120000",
                "index 0000000..1111111",
                "--- /dev/null",
                "+++ b/l",
                "@@ -0,0 +1 @@",
                "+target",
                "\\ No newline at end of file",
                "",
            ]
            .join("\n")
        }

        const SUBMODULE: &str = "diff --git a/sub b/sub\nindex 1111111..2222222 160000\n--- a/sub\n+++ b/sub\n@@ -1 +1 @@\n-Subproject commit 1\n+Subproject commit 2\n";

        fn type_change() -> String {
            format!(
                "{}diff --git a/l b/l\nnew file mode 100644\nindex 0000000..2222222\n--- /dev/null\n+++ b/l\n@@ -0,0 +1 @@\n+one\n",
                link()
            )
        }

        #[test]
        fn stage_a_link_a_submodule_or_a_file_that_changed_type_whole() {
            assert!(staged_whole(&link()));
            assert!(staged_whole(SUBMODULE));
            assert!(staged_whole(&type_change()));
            assert!(!staged_whole(&format!("{HEADER}\n@@ -1 +1 @@\n-a\n+b\n")));
        }

        #[test]
        fn tell_a_link_by_its_mode_in_the_header_not_in_its_lines() {
            assert!(is_link(&link()));
            assert!(!is_link(SUBMODULE));
            assert!(!is_link(&format!("{HEADER}\n@@ -1 +1 @@\n-a\n+ 120000\n")));
            // Git ends its lines with `\n` only: a `\r` is part of a path.
            assert!(!is_link(
                "diff --git a/x 120000\r b/x 120000\r\nnew file mode 100644\n"
            ));
        }
    }
}
