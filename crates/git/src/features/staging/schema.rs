//! The lines of a file's changes to stage or unstage. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::core::errors::{Error, Result};

/// Lines of one side of a file, by their numbers, from `start` to `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRange {
    pub start: u64,
    pub end: u64,
}

/// Lines picked from a file's patch, by their numbers as its hunk headers count them: removed ones
/// in the file's old side, added ones in its new side. Unchanged lines are left out, as there's
/// nothing to stage in them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineSelection {
    pub deletions: Vec<LineRange>,
    pub additions: Vec<LineRange>,
}

impl LineSelection {
    /// Refuses a line numbered 0, as the schema's `z.number().int().min(1)` did.
    pub fn check(&self) -> Result<()> {
        let numbered = self
            .deletions
            .iter()
            .chain(&self.additions)
            .all(|range| range.start >= 1 && range.end >= 1);
        if numbered {
            Ok(())
        } else {
            Err(Error::InvalidInput("Lines are numbered from 1.".to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_selection_as_the_ui_sends_it() {
        let selection: LineSelection =
            serde_json::from_str(r#"{"deletions":[{"start":1,"end":2}],"additions":[]}"#).unwrap();
        assert_eq!(selection.deletions, [LineRange { start: 1, end: 2 }]);
        assert!(selection.check().is_ok());
        let zero = LineSelection {
            deletions: vec![],
            additions: vec![LineRange { start: 0, end: 1 }],
        };
        assert!(matches!(zero.check(), Err(Error::InvalidInput(_))));
    }
}
