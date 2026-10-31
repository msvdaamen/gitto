//! From `git/patch.ts` (see the module's doc).

/// Whether `patch` has lines to stage or unstage left, rather than none, or a mode change.
pub fn has_hunks(patch: &str) -> bool {
    patch.contains("\n@@ ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_a_patch_with_lines_left() {
        assert!(has_hunks(
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-a\n+b\n"
        ));
        assert!(!has_hunks(
            "diff --git a/a b/a\nold mode 100644\nnew mode 100755\n"
        ));
        assert!(!has_hunks(""));
    }
}
