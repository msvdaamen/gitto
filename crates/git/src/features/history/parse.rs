//! Reading the log. Ported from `history/parse.ts`.

use super::schema::{Commit, CommitRef, CommitRefKind};
use crate::features::refs::parse::parse_ref_name;

/// What the log prints of each commit: SHA, parents, author name, author email, author date,
/// commit date, decorations, subject and body.
const FIELDS: usize = 9;

/// With -z, commits are NUL-separated as well, so the output is a flat list of fields where every
/// [`FIELDS`] entries make up one commit. None of the fields can contain NUL.
pub const LOG_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%ct%x00%D%x00%s%x00%b";

/// Seconds since the epoch, as the log prints them, in milliseconds; 0 for none (JS's `Number("")`).
pub(crate) fn millis(seconds: &str) -> i64 {
    seconds
        .trim()
        .parse::<i64>()
        .unwrap_or(0)
        .saturating_mul(1000)
}

/// The commits in the output of `log -z` with [`LOG_FORMAT`].
pub fn parse_log(output: &str) -> Vec<Commit> {
    let fields: Vec<&str> = output.split('\0').collect();
    fields
        .chunks_exact(FIELDS)
        .map(|commit| {
            let [
                sha,
                parents,
                author_name,
                author_email,
                authored_at,
                committed_at,
                refs,
                subject,
                body,
            ] = commit
            else {
                unreachable!("chunks of {FIELDS}")
            };
            Commit {
                sha: sha.trim().to_owned(),
                parents: if parents.is_empty() {
                    Vec::new()
                } else {
                    parents.split(' ').map(str::to_owned).collect()
                },
                author_name: (*author_name).to_owned(),
                author_email: (*author_email).to_owned(),
                authored_at: millis(authored_at),
                committed_at: millis(committed_at),
                refs: parse_decorations(refs),
                subject: (*subject).to_owned(),
                body: body.trim().to_owned(),
            }
        })
        .collect()
}

/// `HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1` → the checked-out local
/// `main`, remote `origin/main` and tag `v1`. A detached HEAD is a bare `HEAD`, even on a commit a
/// branch points at too (`HEAD, refs/heads/main`). Needs `--decorate=full`: short names can't tell
/// a remote branch from a local one named like it (`origin/main`).
fn parse_decorations(decorations: &str) -> Vec<CommitRef> {
    if decorations.is_empty() {
        return Vec::new();
    }
    decorations
        .split(", ")
        .filter_map(|decoration| {
            if decoration == "HEAD" {
                return Some(CommitRef {
                    kind: CommitRefKind::Head,
                    name: "HEAD".to_owned(),
                    full_name: "HEAD".to_owned(),
                    current: false,
                });
            }
            let checked_out = decoration.strip_prefix("HEAD -> ");
            let full_name = checked_out
                .or_else(|| decoration.strip_prefix("tag: "))
                .unwrap_or(decoration);
            let parsed = parse_ref_name(full_name)?;
            Some(CommitRef {
                kind: parsed.kind.into(),
                name: parsed.name,
                full_name: full_name.to_owned(),
                current: checked_out.is_some(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_ref(kind: CommitRefKind, name: &str, full_name: &str, current: bool) -> CommitRef {
        CommitRef {
            kind,
            name: name.to_owned(),
            full_name: full_name.to_owned(),
            current,
        }
    }

    #[test]
    fn reads_nul_separated_commits() {
        // sha, parents, author name, author email, author date, commit date, decorations, subject,
        // body
        let decorations = [
            "HEAD -> refs/heads/main",
            "refs/remotes/origin/main",
            "refs/remotes/origin/HEAD",
            "refs/heads/origin/local",
            "tag: refs/tags/v1",
            "refs/stash",
        ]
        .join(", ");
        let commits = [
            [
                "bbb",
                "aaa ccc",
                "Ada Lovelace",
                "ada@example.com",
                "1700000000",
                "1700000500",
                &decorations,
                "Merge branch 'feature'",
                "Body line\n\n",
            ],
            [
                "aaa",
                "",
                "Grace Hopper",
                "grace@example.com",
                "1600000000",
                "1600000000",
                "",
                "Initial",
                "",
            ],
        ];
        let output = commits.concat().join("\0");

        assert_eq!(
            parse_log(&output),
            [
                Commit {
                    sha: "bbb".into(),
                    parents: vec!["aaa".into(), "ccc".into()],
                    author_name: "Ada Lovelace".into(),
                    author_email: "ada@example.com".into(),
                    authored_at: 1_700_000_000_000,
                    committed_at: 1_700_000_500_000,
                    refs: vec![
                        commit_ref(CommitRefKind::Local, "main", "refs/heads/main", true),
                        commit_ref(
                            CommitRefKind::Remote,
                            "origin/main",
                            "refs/remotes/origin/main",
                            false
                        ),
                        // A local branch that only looks like a remote one.
                        commit_ref(
                            CommitRefKind::Local,
                            "origin/local",
                            "refs/heads/origin/local",
                            false
                        ),
                        commit_ref(CommitRefKind::Tag, "v1", "refs/tags/v1", false),
                    ],
                    subject: "Merge branch 'feature'".into(),
                    body: "Body line".into(),
                },
                Commit {
                    sha: "aaa".into(),
                    parents: vec![],
                    author_name: "Grace Hopper".into(),
                    author_email: "grace@example.com".into(),
                    authored_at: 1_600_000_000_000,
                    committed_at: 1_600_000_000_000,
                    refs: vec![],
                    subject: "Initial".into(),
                    body: "".into(),
                },
            ]
        );
    }

    #[test]
    fn tells_a_detached_head_from_a_checked_out_branch_on_the_same_commit() {
        let output = [
            "aaa",
            "",
            "Ada",
            "ada@example.com",
            "1",
            "1",
            "HEAD, refs/heads/main",
            "S",
            "",
        ];

        assert_eq!(
            parse_log(&output.join("\0"))[0].refs,
            [
                commit_ref(CommitRefKind::Head, "HEAD", "HEAD", false),
                commit_ref(CommitRefKind::Local, "main", "refs/heads/main", false),
            ]
        );
    }

    #[test]
    fn returns_nothing_for_an_empty_log() {
        assert_eq!(parse_log(""), []);
    }

    #[test]
    fn leaves_current_out_unless_its_the_checked_out_branch() {
        let refs = parse_decorations("HEAD -> refs/heads/main, refs/heads/side");
        let json = serde_json::to_value(&refs).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                { "kind": "local", "name": "main", "fullName": "refs/heads/main", "current": true },
                { "kind": "local", "name": "side", "fullName": "refs/heads/side" },
            ])
        );
        // And read back as it was.
        assert_eq!(
            serde_json::from_value::<Vec<CommitRef>>(json).unwrap(),
            refs
        );
    }
}
