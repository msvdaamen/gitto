//! The lines of a file's changes to stage or unstage. Ported from `schema.ts`.

use std::num::NonZeroU64;

use crate::core::errors::{Error, Result};

/// Lines of one side of a file, by their numbers, from `start` to `end`: numbered from 1, as the
/// schema's `z.number().int().min(1)` has them, and `start` isn't after `end`. Checked when it's
/// made with `new`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    start: NonZeroU64,
    end: NonZeroU64,
}

impl LineRange {
    /// The lines `start` to `end`; `Error::InvalidInput` if either is 0, or `start` is after
    /// `end`.
    pub fn new(start: u64, end: u64) -> Result<LineRange> {
        let (Some(start), Some(end)) = (NonZeroU64::new(start), NonZeroU64::new(end)) else {
            return Err(Error::InvalidInput("Lines are numbered from 1.".to_owned()));
        };
        if start > end {
            return Err(Error::InvalidInput(format!(
                "The lines end before they start. ({start} to {end})"
            )));
        }
        Ok(LineRange { start, end })
    }

    pub fn start(self) -> u64 {
        self.start.get()
    }

    pub fn end(self) -> u64 {
        self.end.get()
    }
}

/// Lines picked from a file's patch, by their numbers as its hunk headers count them: removed ones
/// in the file's old side, added ones in its new side. Unchanged lines are left out, as there's
/// nothing to stage in them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineSelection {
    pub deletions: Vec<LineRange>,
    pub additions: Vec<LineRange>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_numbered_from_one() {
        assert!(LineRange::new(1, 1).is_ok());
        let error = LineRange::new(0, 1).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
        assert_eq!(error.to_string(), "Lines are numbered from 1.");
        assert!(LineRange::new(2, 0).is_err());
    }

    #[test]
    fn lines_dont_end_before_they_start() {
        assert_eq!(LineRange::new(2, 3).unwrap().end(), 3);
        let error = LineRange::new(3, 2).unwrap_err();
        assert!(matches!(error, Error::InvalidInput(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            "The lines end before they start. (3 to 2)"
        );
    }
}
