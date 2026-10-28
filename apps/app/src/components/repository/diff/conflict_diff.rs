//! A conflicted file's text, conflict markers and all, read as its conflicts: each one's sides, ours
//! and theirs, and the base's lines between them with diff3 (or zdiff3) markers, among the lines
//! both sides have. Resolving one rewrites its region of the text. Ported from `conflict-diff.ts`,
//! with what it had @pierre/diffs do (`parseMergeConflictDiffFromFile`, `resolveConflict`): the
//! markers are read as the library read them, and the text is kept as the library kept it, as two
//! sides, ours (with the base's lines) and theirs (with them too), which are highlighted whole.

use std::ops::Range;
use std::sync::Arc;

use gitto_ui::diff::diff_metadata::{ChangeType, FileDiff, Lines};

/// Unchanged lines shown around each conflict, as many as git shows around a change.
pub const CONTEXT_LINES: usize = 3;

/// How a conflict is resolved: ours, theirs, or both, ours first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Keep ours (the library's `current`).
    Ours,
    /// Keep theirs (the library's `incoming`).
    Theirs,
    /// Keep both, ours first.
    Both,
}

impl Resolution {
    fn as_str(self) -> &'static str {
        match self {
            Resolution::Ours => "current",
            Resolution::Theirs => "incoming",
            Resolution::Both => "both",
        }
    }
}

/// A conflict marker's kind (the library's marker rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    /// `<<<<<<< HEAD`, before ours.
    Start,
    /// `||||||| base`, before the base's lines (diff3).
    Base,
    /// `=======`, between the sides.
    Separator,
    /// `>>>>>>> feature`, after theirs.
    End,
}

/// One conflict: its markers, as they are in the file, and where its lines are in the sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRegion {
    /// The marker lines, line ends and all.
    pub start_marker: String,
    pub base_marker: Option<String>,
    pub separator: String,
    pub end_marker: String,
    /// The line its start marker is on in the file, from 0.
    pub start_line: usize,
    /// Its lines on our side, by their indexes in [`ConflictState::ours`].
    pub ours: Range<usize>,
    /// The base's lines, with diff3 markers: in ours, and in theirs, which both have them.
    pub base: Option<(Range<usize>, Range<usize>)>,
    /// Its lines on their side, by their indexes in [`ConflictState::theirs`].
    pub theirs: Range<usize>,
}

impl ConflictRegion {
    /// The marker line of `kind`, if it has one.
    pub fn marker(&self, kind: MarkerKind) -> Option<&str> {
        match kind {
            MarkerKind::Start => Some(&self.start_marker),
            MarkerKind::Base => self.base_marker.as_deref(),
            MarkerKind::Separator => Some(&self.separator),
            MarkerKind::End => Some(&self.end_marker),
        }
    }
}

/// A run of the file: lines both sides have, or a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Lines of the file outside the conflicts, by their indexes in the file, and where they start
    /// in each side.
    Unchanged {
        lines: Range<usize>,
        ours_at: usize,
        theirs_at: usize,
    },
    /// The conflict by its index (see [`ConflictState::conflicts`]).
    Conflict(usize),
    /// The lines a conflict was resolved with, by its index, as [`Segment::Unchanged`] has them:
    /// still shown, with the lines around them, where the conflict was.
    Resolved {
        index: usize,
        lines: Range<usize>,
        ours_at: usize,
        theirs_at: usize,
    },
}

/// A conflicted file's text, and its conflicts (the library's `ConflictState`).
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictState {
    /// The text on disk, or as it'll be written, with what's left of the markers, by line.
    pub file: Arc<Lines>,
    pub segments: Vec<Segment>,
    /// Each conflict, by its index; `None` once it's resolved. The indexes stay as conflicts are
    /// resolved, so what's asked of one is of the same one.
    pub conflicts: Vec<Option<ConflictRegion>>,
    /// Both sides as a diff (ours the deletions, theirs the additions), to highlight them, named
    /// in the highlighting cache.
    pub diff: Arc<FileDiff>,
}

impl ConflictState {
    /// The text, conflict markers and all.
    pub fn contents(&self) -> &str {
        self.file.text()
    }

    /// Our side: the lines outside the conflicts, ours, and the base's.
    pub fn ours(&self) -> &Lines {
        &self.diff.deletion_lines
    }

    /// Their side: the lines outside the conflicts, the base's, and theirs.
    pub fn theirs(&self) -> &Lines {
        &self.diff.addition_lines
    }

    /// The conflict `index`, unless it's resolved.
    pub fn conflict(&self, index: usize) -> Option<&ConflictRegion> {
        self.conflicts.get(index).and_then(Option::as_ref)
    }
}

/// The kind of conflict marker `line` is, if it's one (`getMergeConflictMarkerType`): seven or
/// more of the same marker character, followed by whitespace or nothing; nothing at all for `=`.
fn marker_type(line: &str) -> Option<MarkerKind> {
    let bytes = line.as_bytes();
    let code = *bytes.first()?;
    if !matches!(code, b'<' | b'>' | b'=' | b'|') {
        return None;
    }
    let mut end = bytes.len();
    if end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    if end < 7 {
        return None;
    }
    let length = bytes[..end]
        .iter()
        .take_while(|byte| **byte == code)
        .count();
    if length < 7 {
        return None;
    }
    if code == b'=' {
        return (length == end).then_some(MarkerKind::Separator);
    }
    if length != end && !matches!(bytes[length], b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ') {
        return None;
    }
    Some(match code {
        b'<' => MarkerKind::Start,
        b'>' => MarkerKind::End,
        _ => MarkerKind::Base,
    })
}

/// Which part of a conflict the lines being read are in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Ours,
    Base,
    Theirs,
}

/// A conflict being read.
struct Frame {
    stage: Stage,
    start_line: usize,
    start_marker: String,
    base_marker: Option<String>,
    separator: Option<String>,
    ours_start: usize,
    base_start: Option<(usize, usize)>,
    theirs_start: Option<usize>,
}

/// `contents` read as its conflicts, with both sides; fails if its markers can't be read as
/// conflicts, e.g. one isn't finished, or one is inside another.
struct Parsed {
    file: Lines,
    segments: Vec<Segment>,
    regions: Vec<ConflictRegion>,
    ours: Lines,
    theirs: Lines,
}

fn parse(contents: &str) -> Result<Parsed, String> {
    let file = Lines::of(contents);
    let mut ours = Lines::new();
    let mut theirs = Lines::new();
    let mut segments = Vec::new();
    let mut regions = Vec::new();
    let mut frame: Option<Frame> = None;
    // The run of unchanged lines being read: where it starts in the file and the sides.
    let mut unchanged: Option<(usize, usize, usize)> = None;
    let end_unchanged =
        |unchanged: &mut Option<(usize, usize, usize)>, segments: &mut Vec<Segment>, at: usize| {
            if let Some((start, ours_at, theirs_at)) = unchanged.take() {
                segments.push(Segment::Unchanged {
                    lines: start..at,
                    ours_at,
                    theirs_at,
                });
            }
        };
    for (index, line) in file.iter().enumerate() {
        let kind = marker_type(line);
        let Some(current) = frame.as_mut() else {
            if kind == Some(MarkerKind::Start) {
                end_unchanged(&mut unchanged, &mut segments, index);
                frame = Some(Frame {
                    stage: Stage::Ours,
                    start_line: index,
                    start_marker: line.to_owned(),
                    base_marker: None,
                    separator: None,
                    ours_start: ours.len(),
                    base_start: None,
                    theirs_start: None,
                });
            } else {
                unchanged.get_or_insert((index, ours.len(), theirs.len()));
                ours.push(line);
                theirs.push(line);
            }
            continue;
        };
        match kind {
            Some(MarkerKind::Start) => {
                return Err("A conflict starts inside another.".to_owned());
            }
            // A second base marker, or one after the separator, or a second separator (a side's
            // line that looks like one, as a heading's underline can): which lines are whose can't
            // be told, and resolving would drop some.
            Some(MarkerKind::Base) if current.stage != Stage::Ours => {
                return Err(format!(
                    "Conflict {} has a base marker out of place.",
                    regions.len()
                ));
            }
            Some(MarkerKind::Separator) if current.stage == Stage::Theirs => {
                return Err(format!(
                    "Conflict {} has more than one separator marker.",
                    regions.len()
                ));
            }
            Some(MarkerKind::Base) => {
                current.stage = Stage::Base;
                current.base_marker = Some(line.to_owned());
                current.base_start = Some((ours.len(), theirs.len()));
            }
            Some(MarkerKind::Separator) => {
                current.stage = Stage::Theirs;
                current.separator = Some(line.to_owned());
                current.theirs_start = Some(theirs.len());
            }
            Some(MarkerKind::End) => {
                let done = frame.take().expect("a conflict being read");
                let Some(separator) = done.separator else {
                    return Err(format!(
                        "Conflict {} is missing a separator marker.",
                        regions.len()
                    ));
                };
                let theirs_start = done.theirs_start.unwrap_or(theirs.len());
                let ours_end = done.base_start.map_or(ours.len(), |(at, _)| at);
                let base = done.base_start.map(|(ours_at, theirs_at)| {
                    let count = ours.len() - ours_at;
                    (ours_at..ours.len(), theirs_at..theirs_at + count)
                });
                regions.push(ConflictRegion {
                    start_marker: done.start_marker,
                    base_marker: done.base_marker,
                    separator,
                    end_marker: line.to_owned(),
                    start_line: done.start_line,
                    ours: done.ours_start..ours_end,
                    base,
                    theirs: theirs_start..theirs.len(),
                });
                segments.push(Segment::Conflict(regions.len() - 1));
            }
            None => match current.stage {
                Stage::Ours => ours.push(line),
                // The base's lines are both sides' (the library's context lines).
                Stage::Base => {
                    ours.push(line);
                    theirs.push(line);
                }
                Stage::Theirs => theirs.push(line),
            },
        }
    }
    if frame.is_some() {
        return Err("A conflict marker isn't finished.".to_owned());
    }
    end_unchanged(&mut unchanged, &mut segments, file.len());
    Ok(Parsed {
        file,
        segments,
        regions,
        ours,
        theirs,
    })
}

/// Both sides as a diff, to highlight them whole, named `cache_key`.
fn sides_diff(name: &str, ours: Lines, theirs: Lines, cache_key: &str) -> FileDiff {
    FileDiff {
        name: name.to_owned(),
        prev_name: None,
        kind: ChangeType::Change,
        hunks: Vec::new(),
        deletion_lines: Arc::new(ours),
        addition_lines: Arc::new(theirs),
        prev_object_id: None,
        new_object_id: None,
        mode: None,
        prev_mode: None,
        is_partial: false,
        cache_key: Some(cache_key.to_owned()),
    }
}

/// `contents`' conflicts, named `cache_key` in the highlighting cache; `None` if it has none.
/// Fails if its markers can't be read as conflicts, e.g. one isn't finished.
pub fn parse_conflicts(
    name: &str,
    contents: &str,
    cache_key: &str,
) -> Result<Option<ConflictState>, String> {
    let parsed = parse(contents)?;
    if parsed.regions.is_empty() {
        return Ok(None);
    }
    Ok(Some(ConflictState {
        diff: Arc::new(sides_diff(name, parsed.ours, parsed.theirs, cache_key)),
        file: Arc::new(parsed.file),
        segments: parsed.segments,
        conflicts: parsed.regions.into_iter().map(Some).collect(),
    }))
}

/// A conflicted file's conflicts, to show one by one, or why they can't be: the file is shown whole
/// then, to edit by hand. Neither for a file without conflicts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadConflicts {
    pub state: Option<ConflictState>,
    pub problem: Option<String>,
}

/// `contents`' conflicts (see [`parse_conflicts`]), or why they can't be shown one by one.
pub fn read_conflicts(name: &str, contents: &str, cache_key: &str) -> ReadConflicts {
    match parse_conflicts(name, contents, cache_key) {
        Ok(state) => ReadConflicts {
            state,
            problem: None,
        },
        Err(_) => ReadConflicts {
            state: None,
            problem: Some(
                "This file's conflict markers can't be read as conflicts. Edit them by hand."
                    .to_owned(),
            ),
        },
    }
}

/// `state` with its conflict `index` resolved as `resolution`: its region of the text rewritten,
/// with ours, theirs, or both (ours first), without the base's lines, and the other conflicts as
/// they were, by the same indexes (`resolveConflict`). Named anew in the highlighting cache, by
/// what it was and what was done to it. `None` if that conflict isn't there.
pub fn resolve(
    state: &ConflictState,
    index: usize,
    resolution: Resolution,
) -> Option<ConflictState> {
    let resolved = state.conflict(index)?;
    let mut contents = String::with_capacity(state.contents().len());
    // Where the text of each conflict resolved is in the new text, by line.
    let mut resolved_lines: Vec<(usize, Range<usize>)> = Vec::new();
    let mut line = 0;
    let push = |contents: &mut String, text: &str, line: &mut usize| {
        contents.push_str(text);
        *line += 1;
    };
    for segment in &state.segments {
        match segment {
            Segment::Unchanged { lines, .. } => {
                for ix in lines.clone() {
                    push(&mut contents, state.file.line(ix), &mut line);
                }
            }
            Segment::Resolved { index, lines, .. } => {
                let start = line;
                for ix in lines.clone() {
                    push(&mut contents, state.file.line(ix), &mut line);
                }
                resolved_lines.push((*index, start..line));
            }
            Segment::Conflict(at) if *at == index => {
                let start = line;
                if resolution != Resolution::Theirs {
                    for ix in resolved.ours.clone() {
                        push(&mut contents, state.ours().line(ix), &mut line);
                    }
                }
                if resolution != Resolution::Ours {
                    for ix in resolved.theirs.clone() {
                        push(&mut contents, state.theirs().line(ix), &mut line);
                    }
                }
                resolved_lines.push((index, start..line));
            }
            Segment::Conflict(at) => {
                let Some(region) = state.conflict(*at) else {
                    continue;
                };
                push(&mut contents, &region.start_marker, &mut line);
                for ix in region.ours.clone() {
                    push(&mut contents, state.ours().line(ix), &mut line);
                }
                if let (Some(marker), Some((base, _))) = (&region.base_marker, &region.base) {
                    push(&mut contents, marker, &mut line);
                    for ix in base.clone() {
                        push(&mut contents, state.ours().line(ix), &mut line);
                    }
                }
                push(&mut contents, &region.separator, &mut line);
                for ix in region.theirs.clone() {
                    push(&mut contents, state.theirs().line(ix), &mut line);
                }
                push(&mut contents, &region.end_marker, &mut line);
            }
        }
    }
    let parsed = parse(&contents).ok()?;
    // The conflicts left, in order, keep their indexes.
    let left: Vec<usize> = conflicts_left(Some(state))
        .into_iter()
        .filter(|at| *at != index)
        .collect();
    if left.len() != parsed.regions.len() {
        return None;
    }
    let mut conflicts = vec![None; state.conflicts.len()];
    let mut regions = parsed.regions.into_iter();
    for at in &left {
        conflicts[*at] = regions.next();
    }
    let mut segments = Vec::with_capacity(parsed.segments.len() + resolved_lines.len());
    for segment in parsed.segments {
        match segment {
            Segment::Conflict(ix) => segments.push(Segment::Conflict(left[ix])),
            Segment::Unchanged {
                lines,
                ours_at,
                theirs_at,
            } => split_resolved(lines, ours_at, theirs_at, &resolved_lines, &mut segments),
            Segment::Resolved { .. } => unreachable!("parsing finds no resolved conflicts"),
        }
    }
    let key = format!(
        "{}:resolved:{index}:{}",
        state.diff.cache_key.as_deref().unwrap_or_default(),
        resolution.as_str()
    );
    Some(ConflictState {
        diff: Arc::new(sides_diff(
            &state.diff.name,
            parsed.ours,
            parsed.theirs,
            &key,
        )),
        file: Arc::new(parsed.file),
        segments,
        conflicts,
    })
}

/// The unchanged lines `lines` (starting at `ours_at` and `theirs_at` in the sides) as segments,
/// with the conflicts resolved among them (`resolved`, by their lines) as theirs.
fn split_resolved(
    lines: Range<usize>,
    ours_at: usize,
    theirs_at: usize,
    resolved: &[(usize, Range<usize>)],
    segments: &mut Vec<Segment>,
) {
    let mut at = lines.start;
    let unchanged_to = |end: usize, at: &mut usize, segments: &mut Vec<Segment>| {
        if end > *at {
            let offset = *at - lines.start;
            segments.push(Segment::Unchanged {
                lines: *at..end,
                ours_at: ours_at + offset,
                theirs_at: theirs_at + offset,
            });
            *at = end;
        }
    };
    for (index, range) in resolved {
        // Within these lines; an empty one (a side without lines) where it is, at either end.
        if range.start < lines.start || range.end > lines.end {
            continue;
        }
        unchanged_to(range.start, &mut at, segments);
        let offset = range.start - lines.start;
        segments.push(Segment::Resolved {
            index: *index,
            lines: range.clone(),
            ours_at: ours_at + offset,
            theirs_at: theirs_at + offset,
        });
        at = range.end;
    }
    unchanged_to(lines.end, &mut at, segments);
}

/// Both sides of `contents` in full, ours and theirs, the base's lines in both (`wholeSides`).
/// Fails if its markers can't be read as conflicts.
pub fn whole_sides(contents: &str) -> Result<(String, String), String> {
    let parsed = parse(contents)?;
    Ok((
        parsed.ours.text().to_owned(),
        parsed.theirs.text().to_owned(),
    ))
}

/// Names a conflicted file's text in the highlighting cache: by its version on disk, which names
/// its bytes, or else by what's in it.
pub fn conflicts_cache_key(file_key: &str, contents: &str, version: Option<&str>) -> String {
    match version {
        Some(version) => format!("{file_key}:conflicts:{version}"),
        None => format!(
            "{file_key}:conflicts:edited:{}:{}",
            contents.encode_utf16().count(),
            hash(contents)
        ),
    }
}

/// A short hash of `text`, to name edits whose version on disk isn't known yet: JavaScript's
/// `Math.imul(31, value) + code` over its UTF-16 code units, in base 36, as the TS app had it.
fn hash(text: &str) -> String {
    let mut value: i32 = 0;
    for unit in text.encode_utf16() {
        value = value.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    to_base36(value as u32)
}

fn to_base36(mut value: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).expect("ASCII digits")
}

/// The conflicts left, by their indexes, in the order they're in the file.
pub fn conflicts_left(state: Option<&ConflictState>) -> Vec<usize> {
    let Some(state) = state else {
        return Vec::new();
    };
    state
        .segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Conflict(index) if state.conflict(*index).is_some() => Some(*index),
            _ => None,
        })
        .collect()
}

/// The label git gave a side in a conflict's markers, e.g. `HEAD`, or `side`.
pub fn marker_label(marker: &str) -> String {
    let run = marker
        .bytes()
        .take_while(|byte| matches!(byte, b'<' | b'>' | b'|'))
        .count();
    let rest = if run >= 7 {
        let rest = &marker[run..];
        rest.strip_prefix(char::is_whitespace).unwrap_or(rest)
    } else {
        marker
    };
    rest.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use gitto_ui::diff::diff_metadata::Side;
    use gitto_ui::diff::patch_files::Expanded;

    use super::super::conflict_rows::{ConflictRow, conflict_rows};
    use super::*;

    /// A file of `lines` numbered lines, with a conflict after each of `conflicts`, in git's merge
    /// style, or diff3's with `base`.
    fn file_with(lines: usize, conflicts: &[usize], base: Option<&[&str]>) -> String {
        let mut text = String::new();
        for line in 1..=lines {
            text.push_str(&format!("line {line}\n"));
            if conflicts.contains(&line) {
                text.push_str(&format!("<<<<<<< HEAD\nours {line}\n"));
                if let Some(base) = base {
                    text.push_str("||||||| base\n");
                    for b in base {
                        text.push_str(&format!("{b}\n"));
                    }
                }
                text.push_str(&format!("=======\ntheirs {line}\n>>>>>>> side\n"));
            }
        }
        text
    }

    /// The text of the lines laid out to show `state`, unchanged and changed, in order.
    fn shown_lines(state: &ConflictState) -> Vec<String> {
        conflict_rows(state, &Expanded::new())
            .iter()
            .filter_map(|row| match row {
                ConflictRow::Line { side, index, .. } => Some(match side {
                    Side::Deletions => state.ours().line(*index).to_owned(),
                    Side::Additions => state.theirs().line(*index).to_owned(),
                }),
                _ => None,
            })
            .collect()
    }

    // parseConflicts

    #[test]
    fn keeps_only_the_lines_around_the_conflicts() {
        let state = parse_conflicts("f.txt", &file_with(1000, &[100, 900], None), "f:1")
            .unwrap()
            .unwrap();
        // Three lines either side, and each side's line.
        assert_eq!(
            shown_lines(&state),
            [
                "line 98\n",
                "line 99\n",
                "line 100\n",
                "ours 100\n",
                "theirs 100\n",
                "line 101\n",
                "line 102\n",
                "line 103\n",
                "line 898\n",
                "line 899\n",
                "line 900\n",
                "ours 900\n",
                "theirs 900\n",
                "line 901\n",
                "line 902\n",
                "line 903\n",
            ]
        );
        assert!(state.ours().iter().any(|line| line == "ours 900\n"));
        assert_eq!(conflicts_left(Some(&state)), [0, 1]);
    }

    #[test]
    fn keeps_a_short_file_whole_so_the_lines_after_its_last_conflict_are_counted() {
        let state = parse_conflicts("f.txt", &file_with(100, &[50], None), "f:1")
            .unwrap()
            .unwrap();
        assert_eq!(state.theirs().len(), 101);
        let hidden: Vec<usize> = conflict_rows(&state, &Expanded::new())
            .iter()
            .filter_map(|row| match row {
                ConflictRow::Separator { hidden, .. } => Some(*hidden),
                _ => None,
            })
            .collect();
        // Before the conflict, and the 47 after its three lines of context.
        assert_eq!(hidden, [47, 47]);
    }

    #[test]
    fn reads_a_diff3_conflict_whose_base_is_longer_than_the_context_around_it() {
        let base: Vec<String> = (0..20).map(|i| format!("base {i}")).collect();
        let base: Vec<&str> = base.iter().map(String::as_str).collect();
        let state = parse_conflicts("f.txt", &file_with(50, &[10], Some(&base)), "f:1")
            .unwrap()
            .unwrap();
        assert_eq!(conflicts_left(Some(&state)), [0]);
        let markers: Vec<MarkerKind> = conflict_rows(&state, &Expanded::new())
            .iter()
            .filter_map(|row| match row {
                ConflictRow::Marker { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            markers,
            [
                MarkerKind::Start,
                MarkerKind::Base,
                MarkerKind::Separator,
                MarkerKind::End
            ]
        );
    }

    #[test]
    fn has_nothing_for_a_file_without_conflicts() {
        assert_eq!(parse_conflicts("f.txt", "just\ntext\n", "f:1"), Ok(None));
    }

    #[test]
    fn throws_for_markers_that_arent_finished() {
        assert!(parse_conflicts("f.txt", "a\n<<<<<<< HEAD\nours\n", "f:1").is_err());
    }

    // resolving a conflict

    #[test]
    fn rewrites_its_region_of_the_file_and_leaves_the_others() {
        let contents = file_with(20, &[5, 15], None);
        let state = parse_conflicts("f.txt", &contents, "f:1").unwrap().unwrap();

        let ours = resolve(&state, 0, Resolution::Ours).unwrap();
        assert_eq!(
            ours.contents(),
            contents.replace(
                "<<<<<<< HEAD\nours 5\n=======\ntheirs 5\n>>>>>>> side\n",
                "ours 5\n"
            )
        );
        assert_eq!(conflicts_left(Some(&ours)), [1]);
        assert!(
            resolve(&state, 1, Resolution::Both)
                .unwrap()
                .contents()
                .contains("line 15\nours 15\ntheirs 15\nline 16\n")
        );
        // Names the diff anew, by what it was and what was done to it, in the highlighting cache.
        assert_ne!(ours.diff.cache_key, state.diff.cache_key);
    }

    #[test]
    fn drops_the_bases_lines_with_diff3_markers() {
        let state = parse_conflicts("f.txt", &file_with(10, &[5], Some(&["base 5"])), "f:1")
            .unwrap()
            .unwrap();
        assert_eq!(
            resolve(&state, 0, Resolution::Theirs).unwrap().contents(),
            file_with(10, &[], None).replace("line 5\n", "line 5\ntheirs 5\n")
        );
    }

    // wholeSides

    #[test]
    fn fills_in_a_partial_diff_with_the_whole_of_both_sides() {
        let contents = file_with(2000, &[50], None);
        let (ours, theirs) = whole_sides(&contents).unwrap();
        assert_eq!(
            ours,
            file_with(2000, &[], None).replace("line 50\n", "line 50\nours 50\n")
        );
        assert_eq!(
            theirs,
            file_with(2000, &[], None).replace("line 50\n", "line 50\ntheirs 50\n")
        );
        // The conflict is where it was, with the lines around it from the whole file.
        let state = parse_conflicts("f.txt", &contents, "f:1").unwrap().unwrap();
        assert_eq!(shown_lines(&state)[0], "line 48\n");
    }

    // markerLabel

    #[test]
    fn reads_the_label_git_gave_a_side() {
        assert_eq!(marker_label("<<<<<<< HEAD\n"), "HEAD");
        assert_eq!(
            marker_label(">>>>>>> 1a2b3c4 (Fix the header)\r\n"),
            "1a2b3c4 (Fix the header)"
        );
        assert_eq!(marker_label(">>>>>>>\n"), "");
    }

    // Beyond the TS tests.

    #[test]
    fn names_edits_by_their_length_and_hash_as_the_ts_app_did() {
        // `(Math.imul(31, v) + code) | 0`, then `>>> 0` in base 36: "abc" is 96354.
        assert_eq!(hash("abc"), "22ci");
        assert_eq!(
            conflicts_cache_key("unstaged::a.txt", "abc", None),
            "unstaged::a.txt:conflicts:edited:3:22ci"
        );
        assert_eq!(
            conflicts_cache_key("unstaged::a.txt", "abc", Some("v1")),
            "unstaged::a.txt:conflicts:v1"
        );
    }

    #[test]
    fn reads_markers_as_the_library_did() {
        assert_eq!(marker_type("<<<<<<< HEAD\n"), Some(MarkerKind::Start));
        assert_eq!(marker_type("<<<<<<<\r\n"), Some(MarkerKind::Start));
        assert_eq!(marker_type("<<<<<<<<<< x"), Some(MarkerKind::Start));
        assert_eq!(marker_type("<<<<<<<x\n"), None);
        assert_eq!(marker_type("=======\n"), Some(MarkerKind::Separator));
        assert_eq!(marker_type("======= x\n"), None);
        assert_eq!(marker_type("||||||| base\n"), Some(MarkerKind::Base));
        assert_eq!(marker_type(">>>>>>> side"), Some(MarkerKind::End));
        assert_eq!(marker_type("<<<<<< six\n"), None);
    }

    #[test]
    fn throws_for_a_conflict_whose_lines_cant_be_told_apart_rather_than_drop_some() {
        // A side's line that reads as a separator: what's between the two would be lost.
        let separators = "<<<<<<< HEAD\nours\n=======\nTitle\n=======\ntheirs\n>>>>>>> side\n";
        assert!(parse_conflicts("f.rst", separators, "f:1").is_err());
        let base_after = "<<<<<<< HEAD\nours\n=======\ntheirs\n||||||| base\nb\n>>>>>>> side\n";
        assert!(parse_conflicts("f.txt", base_after, "f:1").is_err());
        let two_bases =
            "<<<<<<< HEAD\nours\n||||||| a\nb\n||||||| c\nd\n=======\nt\n>>>>>>> side\n";
        assert!(parse_conflicts("f.txt", two_bases, "f:1").is_err());
        assert!(read_conflicts("f.rst", separators, "f:1").problem.is_some());
    }

    #[test]
    fn leaves_markers_outside_a_conflict_as_text() {
        let contents = "Title\n=======\n\n>>>>>>> quoted\n";
        assert_eq!(parse_conflicts("f.md", contents, "f:1"), Ok(None));
    }
}
