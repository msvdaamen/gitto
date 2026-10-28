//! NUL-separated records, as git reads paths from its stdin (`--pathspec-file-nul`) and prints them
//! with `-z`.

use crate::core::errors::{Error, Result};

/// `paths`, each ended by a NUL, for git's stdin.
pub fn nul_terminated<S: AsRef<str>>(paths: &[S]) -> String {
    paths
        .iter()
        .map(|path| format!("{}\0", path.as_ref()))
        .collect()
}

/// The NUL-separated names in `output`, as `--name-only -z` prints them.
pub fn split_nul(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A file in `--raw -z` output (see `raw_entries`), as bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEntry<'a> {
    /// Its mode on the old side, `000000` if it isn't there.
    pub old_mode: &'a [u8],
    pub new_mode: &'a [u8],
    /// Its object name on the old side, zeros if it isn't there.
    pub old_object: &'a [u8],
    pub new_object: &'a [u8],
    /// Its status letter, and a rename's or copy's score after it, e.g. `M` or `R100`.
    pub status: &'a [u8],
    /// The path it was renamed or copied from.
    pub orig_path: Option<&'a [u8]>,
    pub path: &'a [u8],
}

/// The entries of `--raw -z` output: `:<mode> <mode> <object> <object> <status>\0<path>\0`, with
/// the path it came from before the path for a rename or copy. One git didn't write that way is a
/// bug (`Error::Bug`), and ends them.
pub fn raw_entries(output: &[u8]) -> impl Iterator<Item = Result<RawEntry<'_>>> {
    let mut records = output.split(|&byte| byte == 0);
    let mut failed = false;
    std::iter::from_fn(move || {
        if failed {
            return None;
        }
        let record = records.next().filter(|record| !record.is_empty())?;
        let entry = raw_entry(record, &mut records);
        failed = entry.is_err();
        Some(entry)
    })
}

/// The entry `record` starts, with its paths from `records`.
fn raw_entry<'a>(
    record: &'a [u8],
    records: &mut impl Iterator<Item = &'a [u8]>,
) -> Result<RawEntry<'a>> {
    let unexpected = || {
        Error::Bug(format!(
            "Unexpected raw diff entry: {}",
            String::from_utf8_lossy(record)
        ))
    };
    let info = record.strip_prefix(b":").ok_or_else(unexpected)?;
    let fields: Vec<&[u8]> = info.split(|&byte| byte == b' ').collect();
    let &[old_mode, new_mode, old_object, new_object, status] = fields.as_slice() else {
        return Err(unexpected());
    };
    let mut path = || {
        records
            .next()
            .filter(|path| !path.is_empty())
            .ok_or_else(unexpected)
    };
    let orig_path = match status.first() {
        Some(b'R' | b'C') => Some(path()?),
        _ => None,
    };
    let path = path()?;
    Ok(RawEntry {
        old_mode,
        new_mode,
        old_object,
        new_object,
        status,
        orig_path,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_paths_with_nuls_and_splits_names_at_them() {
        assert_eq!(nul_terminated(&["a", "b c"]), "a\0b c\0");
        assert_eq!(split_nul("a\0b c\0"), ["a", "b c"]);
        assert_eq!(split_nul(""), Vec::<String>::new());
    }

    #[test]
    fn reads_raw_entries_a_renames_paths_and_all() {
        let output = b":100644 100644 aaa bbb M\0a.txt\0:100644 100644 ccc ccc R100\0old\0new\0:000000 100644 000 ddd A\0caf\xe9\0";
        let entries: Vec<RawEntry> = raw_entries(output).map(Result::unwrap).collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            RawEntry {
                old_mode: b"100644",
                new_mode: b"100644",
                old_object: b"aaa",
                new_object: b"bbb",
                status: b"M",
                orig_path: None,
                path: b"a.txt",
            }
        );
        assert_eq!(entries[1].orig_path, Some(&b"old"[..]));
        assert_eq!(entries[1].path, b"new");
        assert_eq!(entries[2].old_mode, b"000000");
        assert_eq!(entries[2].path, b"caf\xe9");
        assert_eq!(raw_entries(b"").count(), 0);
    }

    #[test]
    fn fails_at_an_entry_git_didnt_write() {
        for output in [&b"a.txt\0"[..], b":100644 aaa M\0a.txt\0", b":1 2 3 4 M\0"] {
            let entries: Vec<_> = raw_entries(output).collect();
            assert!(
                matches!(entries.as_slice(), [Err(Error::Bug(_))]),
                "{entries:?}"
            );
        }
    }
}
