//! What the changes of the branches take (see `super::changes`). Ported from
//! `git/queries/branch.ts`.

/// A branch to make, `name`, at the branch `from`, by its full ref name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBranchFrom {
    pub name: String,
    pub from: String,
}

/// A merge of the branch `reference`, by its full ref name, into the checked-out one, `into`, as
/// the user saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merge {
    pub reference: String,
    pub into: String,
}

/// A rebase of the checked-out branch, `branch`, as the user saw it, onto the branch `reference`,
/// by its full ref name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebase {
    pub reference: String,
    pub branch: String,
}
