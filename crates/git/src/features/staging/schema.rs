//! The lines of a file's changes to stage or unstage. Ported from `schema.ts`.

use serde::{Deserialize, Serialize};

use crate::core::errors::{Error, Result};

/// Lines of one side of a file, by their numbers, from `start` to `end`: numbered from 1, as the
/// schema's `z.number().int().min(1)` has them. Checked when it's made with `new`, or deserialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawLineRange")]
pub struct LineRange {
    start: u64,
    end: u64,
}

impl LineRange {
    /// The lines `start` to `end`; `Error::InvalidInput` if either is 0.
    pub fn new(start: u64, end: u64) -> Result<LineRange> {
        if start >= 1 && end >= 1 {
            Ok(LineRange { start, end })
        } else {
            Err(Error::InvalidInput("Lines are numbered from 1.".to_owned()))
        }
    }

    pub fn start(self) -> u64 {
        self.start
    }

    pub fn end(self) -> u64 {
        self.end
    }
}

/// A [`LineRange`] as it's sent, before it's checked.
#[derive(Deserialize)]
struct RawLineRange {
    start: u64,
    end: u64,
}

impl TryFrom<RawLineRange> for LineRange {
    type Error = Error;

    fn try_from(raw: RawLineRange) -> Result<LineRange> {
        LineRange::new(raw.start, raw.end)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_selection_as_the_ui_sends_it() {
        let selection: LineSelection =
            serde_json::from_str(r#"{"deletions":[{"start":1,"end":2}],"additions":[]}"#).unwrap();
        assert_eq!(selection.deletions, [LineRange::new(1, 2).unwrap()]);
        assert_eq!(
            serde_json::to_string(&selection).unwrap(),
            r#"{"deletions":[{"start":1,"end":2}],"additions":[]}"#
        );
        let zero = serde_json::from_str::<LineSelection>(
            r#"{"deletions":[],"additions":[{"start":0,"end":1}]}"#,
        );
        assert!(zero.is_err());
        let fraction = serde_json::from_str::<LineRange>(r#"{"start":1.5,"end":2}"#);
        assert!(fraction.is_err());
    }

    #[test]
    fn lines_are_numbered_from_one() {
        assert!(LineRange::new(1, 1).is_ok());
        let error = LineRange::new(0, 1).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
        assert_eq!(error.to_string(), "Lines are numbered from 1.");
        assert!(LineRange::new(2, 0).is_err());
    }
}
