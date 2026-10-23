//! Reading `for-each-ref`'s output, and full ref names. Ported from `refs/parse.ts`.

use std::sync::LazyLock;

use regex::Regex;

use super::schema::{Ref, RefKind};

/// Lists the refs for [`parse_refs`]. Ref names can't contain newlines or NUL, so one line per ref
/// with NUL-separated fields is safe.
pub const REFS_ARGS: &[&str] = &[
    "for-each-ref",
    "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(HEAD)",
    "refs/heads",
    "refs/remotes",
    "refs/tags",
];

const PREFIXES: &[(&str, RefKind)] = &[
    ("refs/heads/", RefKind::Local),
    ("refs/remotes/", RefKind::Remote),
    ("refs/tags/", RefKind::Tag),
];

/// The kind and short name of a full ref name (see [`parse_ref_name`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefName {
    pub kind: RefKind,
    /// Short name, e.g. `main`, `origin/main`, `v1`.
    pub name: String,
}

/// The kind and short name of a full ref name, e.g. `refs/remotes/origin/main` → remote
/// `origin/main`. `None` for refs that aren't branches or tags, like `refs/stash`, and for
/// `refs/remotes/<remote>/HEAD`: a symbolic ref to the remote's default branch, not a branch.
pub fn parse_ref_name(full_name: &str) -> Option<RefName> {
    let (name, kind) = PREFIXES
        .iter()
        .find_map(|(prefix, kind)| full_name.strip_prefix(prefix).map(|name| (name, *kind)))?;
    if kind == RefKind::Remote && full_name.ends_with("/HEAD") {
        return None;
    }
    Some(RefName {
        kind,
        name: name.to_owned(),
    })
}

static AHEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ahead ([0-9]+)").unwrap());
static BEHIND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"behind ([0-9]+)").unwrap());

/// The count `pattern` finds in `track`, e.g. 2 in `ahead 2, behind 1`; 0 if there's none.
fn count(pattern: &Regex, track: &str) -> u64 {
    pattern
        .captures(track)
        .and_then(|captures| captures[1].parse().ok())
        .unwrap_or(0)
}

/// The refs in the output of [`REFS_ARGS`].
pub fn parse_refs(output: &str) -> Vec<Ref> {
    let mut refs = Vec::new();

    for line in output.split('\n') {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\0');
        let mut field = || fields.next().unwrap_or("");
        let (full_name, sha, upstream, track, head) = (field(), field(), field(), field(), field());

        let Some(RefName { kind, name }) = parse_ref_name(full_name) else {
            continue;
        };

        refs.push(Ref {
            name,
            full_name: full_name.to_owned(),
            kind,
            sha: sha.to_owned(),
            current: head == "*",
            upstream: (!upstream.is_empty()).then(|| upstream.to_owned()),
            ahead: count(&AHEAD, track),
            behind: count(&BEHIND, track),
        });
    }

    refs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_branches_remotes_and_tags_skipping_remote_heads() {
        let output = [
            [
                "refs/heads/main",
                "aaa",
                "origin/main",
                "ahead 2, behind 1",
                "*",
            ],
            ["refs/heads/feature/x", "bbb", "", "", " "],
            ["refs/remotes/origin/HEAD", "aaa", "", "", " "],
            ["refs/remotes/origin/main", "ccc", "", "", " "],
            ["refs/tags/v1.0.0", "ddd", "", "", " "],
        ]
        .map(|fields| fields.join("\0"))
        .join("\n");

        let refs = parse_refs(&output);
        assert_eq!(refs.len(), 4);
        assert_eq!(
            refs[0],
            Ref {
                name: "main".into(),
                full_name: "refs/heads/main".into(),
                kind: RefKind::Local,
                sha: "aaa".into(),
                current: true,
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 1,
            }
        );
        let second = &refs[1];
        assert_eq!(
            (
                second.name.as_str(),
                second.kind,
                second.current,
                second.upstream.as_deref()
            ),
            ("feature/x", RefKind::Local, false, None)
        );
        assert_eq!(
            (refs[2].name.as_str(), refs[2].kind),
            ("origin/main", RefKind::Remote)
        );
        assert_eq!(
            (refs[3].name.as_str(), refs[3].kind),
            ("v1.0.0", RefKind::Tag)
        );
    }
}
