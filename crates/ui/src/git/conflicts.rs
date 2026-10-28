//! Ported from `git/conflicts.ts`: what a conflicted file is, and what's under way in the
//! repository, in the user's words.

use gitto_git::Version;
use gitto_git::features::conflicts::schema::{Conflict, ConflictSide, ConflictSides, Side};
use gitto_git::features::operation::schema::{CommitName, Operation};

use super::short_sha;

/// What one side of a conflict has at the file's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideKind {
    File,
    Link,
    Submodule,
}

pub fn side_kind(side: &ConflictSide) -> SideKind {
    match side.mode.as_str() {
        "120000" => SideKind::Link,
        "160000" => SideKind::Submodule,
        _ => SideKind::File,
    }
}

/// How a conflicted file is resolved: in its text, conflict by conflict, when both sides changed it
/// as a text file; or by keeping one side whole otherwise, e.g. when one deleted it, or it's binary;
/// or not at all, once it isn't conflicted any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    Text { contents: String, version: Version },
    Sides,
    Resolved,
}

pub fn conflict_kind(conflict: &Conflict) -> ConflictKind {
    let Conflict {
        base,
        ours,
        theirs,
        text,
        ..
    } = conflict;
    if base.is_none() && ours.is_none() && theirs.is_none() {
        return ConflictKind::Resolved;
    }
    match (ours, theirs, text) {
        (Some(ours), Some(theirs), Some(text))
            if side_kind(ours) == SideKind::File && side_kind(theirs) == SideKind::File =>
        {
            ConflictKind::Text {
                contents: text.contents.clone(),
                version: text.version.clone(),
            }
        }
        _ => ConflictKind::Sides,
    }
}

/// What each side did to the file, e.g. "Deleted in theirs, changed in ours".
pub fn describe_conflict(sides: &ConflictSides) -> &'static str {
    let base = sides.base.is_some();
    match (sides.ours.is_some(), sides.theirs.is_some()) {
        (true, true) if base => "Changed on both sides",
        (true, true) => "Added on both sides",
        (true, false) if base => "Deleted in theirs, changed in ours",
        (true, false) => "Added in ours, not in theirs",
        (false, true) if base => "Deleted in ours, changed in theirs",
        (false, true) => "Added in theirs, not in ours",
        (false, false) => "Deleted on both sides",
    }
}

/// What a side has, e.g. "A submodule at 1a2b3c4", or "Deleted".
pub fn describe_side(side: Option<&ConflictSide>) -> String {
    let Some(side) = side else {
        return "Deleted".to_owned();
    };
    match side_kind(side) {
        SideKind::Link => "A symbolic link".to_owned(),
        SideKind::Submodule => format!("A submodule at {}", short_sha(&side.oid)),
        SideKind::File if side.mode == "100755" => "An executable file".to_owned(),
        SideKind::File => "A file".to_owned(),
    }
}

/// The button that keeps a side whole: "Keep ours", or "Delete, as theirs does".
pub fn keep_label(sides: &ConflictSides, side: Side) -> String {
    let kept = match side {
        Side::Ours => &sides.ours,
        Side::Theirs => &sides.theirs,
    };
    if kept.is_some() {
        format!("Keep {}", side.as_str())
    } else {
        format!("Delete, as {} does", side.as_str())
    }
}

/// What's under way, e.g. "Merging feature into main", or "Rebasing feature onto main".
pub fn operation_title(operation: &Operation) -> String {
    match operation {
        Operation::Merge { merging, into } => match into.as_deref().filter(|into| !into.is_empty())
        {
            Some(into) => format!("Merging {merging} into {into}"),
            None => format!("Merging {merging}"),
        },
        Operation::Rebase { branch, onto, .. } => {
            format!(
                "Rebasing {} onto {onto}",
                branch.as_deref().unwrap_or("HEAD")
            )
        }
        Operation::CherryPick { commit, .. } => picking("Cherry-picking", commit.as_ref()),
        Operation::Revert { commit, .. } => picking("Reverting", commit.as_ref()),
        Operation::Am { .. } => "Applying patches".to_owned(),
    }
}

/// A cherry-pick's or revert's title: `verb` and the commit it's at, e.g. "Reverting 1a2b3c4 Fix
/// it"; `None` between the commits of a series, once one was committed by hand.
fn picking(verb: &str, commit: Option<&CommitName>) -> String {
    match commit {
        Some(commit) => format!("{verb} {} {}", commit.sha, commit.subject)
            .trim_end()
            .to_owned(),
        None => format!("{verb} commits"),
    }
}

/// How far it's got, e.g. "3/7", or "2 more to go"; `None` if that isn't known.
pub fn operation_progress(operation: &Operation) -> Option<String> {
    match operation {
        Operation::Rebase { steps, .. } | Operation::Am { steps } => steps
            .as_ref()
            .map(|steps| format!("{}/{}", steps.step, steps.total)),
        Operation::CherryPick { remaining, .. } | Operation::Revert { remaining, .. } => {
            (*remaining > 0).then(|| format!("{remaining} more to go"))
        }
        Operation::Merge { .. } => None,
    }
}

/// The operation's name, to ask whether to abort it: "the merge", say.
pub fn operation_name(operation: &Operation) -> String {
    match operation {
        Operation::Am { .. } => "applying the patches".to_owned(),
        _ => format!("the {}", operation.kind().as_str()),
    }
}

#[cfg(test)]
mod tests {
    use gitto_git::features::conflicts::schema::ConflictText;
    use gitto_git::features::operation::schema::Steps;

    use super::*;

    fn side(mode: &str, oid: &str) -> ConflictSide {
        ConflictSide {
            mode: mode.into(),
            oid: oid.into(),
        }
    }

    fn file() -> Option<ConflictSide> {
        Some(side("100644", &"a".repeat(40)))
    }

    fn conflict() -> Conflict {
        Conflict {
            base: file(),
            ours: file(),
            theirs: file(),
            text: Some(ConflictText {
                contents: "text".into(),
                version: "v1".into(),
            }),
            version: Some("v1".into()),
            binary: false,
            unreadable: None,
        }
    }

    fn sides(
        base: Option<ConflictSide>,
        ours: Option<ConflictSide>,
        theirs: Option<ConflictSide>,
    ) -> ConflictSides {
        ConflictSides { base, ours, theirs }
    }

    mod conflict_kind {
        use super::*;

        #[test]
        fn resolves_a_text_file_changed_on_both_sides_in_its_text() {
            assert_eq!(
                conflict_kind(&conflict()),
                ConflictKind::Text {
                    contents: "text".into(),
                    version: "v1".into()
                }
            );
            let added = Conflict {
                base: None,
                ..conflict()
            };
            assert!(matches!(conflict_kind(&added), ConflictKind::Text { .. }));
        }

        #[test]
        fn keeps_a_side_whole_otherwise() {
            let deleted = Conflict {
                theirs: None,
                ..conflict()
            };
            assert_eq!(conflict_kind(&deleted), ConflictKind::Sides);
            let binary = Conflict {
                text: None,
                binary: true,
                ..conflict()
            };
            assert_eq!(conflict_kind(&binary), ConflictKind::Sides);
            let submodule = Conflict {
                ours: Some(side("160000", &"b".repeat(40))),
                ..conflict()
            };
            assert_eq!(conflict_kind(&submodule), ConflictKind::Sides);
        }

        #[test]
        fn has_nothing_to_resolve_once_it_isnt_conflicted() {
            let resolved = Conflict {
                base: None,
                ours: None,
                theirs: None,
                ..conflict()
            };
            assert_eq!(conflict_kind(&resolved), ConflictKind::Resolved);
        }
    }

    mod what_each_side_did {
        use super::*;

        #[test]
        fn says_which_side_deleted_or_added_the_file() {
            assert_eq!(
                describe_conflict(&sides(file(), file(), file())),
                "Changed on both sides"
            );
            assert_eq!(
                describe_conflict(&sides(None, file(), file())),
                "Added on both sides"
            );
            assert_eq!(
                describe_conflict(&sides(file(), file(), None)),
                "Deleted in theirs, changed in ours"
            );
            assert_eq!(
                describe_conflict(&sides(file(), None, file())),
                "Deleted in ours, changed in theirs"
            );
            assert_eq!(
                describe_conflict(&sides(None, None, file())),
                "Added in theirs, not in ours"
            );
            assert_eq!(
                describe_conflict(&sides(file(), None, None)),
                "Deleted on both sides"
            );
        }

        #[test]
        fn says_what_a_side_has_and_what_keeping_it_does() {
            assert_eq!(describe_side(None), "Deleted");
            assert_eq!(describe_side(Some(&side("120000", "c"))), "A symbolic link");
            assert_eq!(
                describe_side(Some(&side("160000", "1a2b3c4d5e"))),
                "A submodule at 1a2b3c4"
            );
            assert_eq!(
                keep_label(&sides(file(), file(), None), Side::Ours),
                "Keep ours"
            );
            assert_eq!(
                keep_label(&sides(file(), file(), None), Side::Theirs),
                "Delete, as theirs does"
            );
        }
    }

    mod whats_under_way {
        use super::*;

        #[test]
        fn names_it_and_how_far_its_got() {
            assert_eq!(
                operation_title(&Operation::Merge {
                    merging: "feature".into(),
                    into: Some("main".into())
                }),
                "Merging feature into main"
            );
            assert_eq!(
                operation_title(&Operation::Merge {
                    merging: "1a2b3c4".into(),
                    into: None
                }),
                "Merging 1a2b3c4"
            );
            let rebase = Operation::Rebase {
                branch: Some("feature".into()),
                onto: "origin/main".into(),
                steps: Some(Steps { step: 3, total: 7 }),
            };
            assert_eq!(
                operation_title(&rebase),
                "Rebasing feature onto origin/main"
            );
            assert_eq!(operation_progress(&rebase).as_deref(), Some("3/7"));
            let pick =
                |commit: Option<CommitName>, remaining| Operation::CherryPick { commit, remaining };
            let commit = || {
                Some(CommitName {
                    sha: "1a2b3c4".into(),
                    subject: "Fix it".into(),
                })
            };
            assert_eq!(
                operation_title(&pick(commit(), 2)),
                "Cherry-picking 1a2b3c4 Fix it"
            );
            assert_eq!(
                operation_progress(&pick(commit(), 2)).as_deref(),
                Some("2 more to go")
            );
            assert_eq!(operation_progress(&pick(commit(), 0)), None);
            // Between the commits of a series, once one was committed by hand.
            assert_eq!(operation_title(&pick(None, 2)), "Cherry-picking commits");
        }
    }

    #[test]
    fn names_the_operation_to_abort() {
        let merge = Operation::Merge {
            merging: "feature".into(),
            into: None,
        };
        assert_eq!(operation_name(&merge), "the merge");
        assert_eq!(
            operation_name(&Operation::CherryPick {
                commit: None,
                remaining: 0
            }),
            "the cherry-pick"
        );
        assert_eq!(
            operation_name(&Operation::Am { steps: None }),
            "applying the patches"
        );
    }
}
