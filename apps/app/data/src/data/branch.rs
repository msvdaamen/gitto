//! What the changes of the branches take (see `super::changes`). Ported from
//! `git/queries/branch.ts`.

use gitto_git::Result;
use gitto_git::input::{BranchName, BranchRef, NonEmpty};

/// A branch to make, `name`, at the branch `from`, by its full ref name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBranchFrom {
    pub name: BranchName,
    pub from: BranchRef,
}

impl NewBranchFrom {
    /// The branch `name`, at `from`, if both are valid.
    pub fn parse(name: impl Into<String>, from: impl Into<String>) -> Result<Self> {
        Ok(NewBranchFrom {
            name: BranchName::parse(name)?,
            from: BranchRef::parse(from)?,
        })
    }
}

/// A merge of the branch `reference`, by its full ref name, into the checked-out one, `into`, as
/// the user saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub reference: BranchRef,
    pub into: NonEmpty,
}

impl Merge {
    /// The merge of `reference` into `into`, if both are valid.
    pub fn parse(reference: impl Into<String>, into: impl Into<String>) -> Result<Self> {
        Ok(Merge {
            reference: BranchRef::parse(reference)?,
            into: NonEmpty::parse(into)?,
        })
    }
}

/// A rebase of the checked-out branch, `branch`, as the user saw it, onto the branch `reference`,
/// by its full ref name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebase {
    pub reference: BranchRef,
    pub branch: NonEmpty,
}

impl Rebase {
    /// The rebase of `branch` onto `reference`, if both are valid.
    pub fn parse(reference: impl Into<String>, branch: impl Into<String>) -> Result<Self> {
        Ok(Rebase {
            reference: BranchRef::parse(reference)?,
            branch: NonEmpty::parse(branch)?,
        })
    }
}
