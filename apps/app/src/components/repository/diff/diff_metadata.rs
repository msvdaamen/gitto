//! A file's patch, parsed: its hunks, and the lines of both its sides. What the TS viewer took
//! from @pierre/diffs (`FileDiffMetadata`, `parsePatchFiles`, `hydratePartialDiff`,
//! `parseDiffFromFile`, `getLineIndexForDiff`), as the rest of the diff view reads it: the line
//! staging (`line_staging`), fitting whole files to a patch (`patch_files`) and the viewer's rows
//! (`viewer_rows`) are ported against this model.
//!
//! A patch as git gives it only has the changed lines and a few around them: such a diff is
//! *partial*, and its sides' lines are the patch's, in order. Filled in with both files read whole
//! ([`hydrate`]), the sides are the files' lines, and the hunks index into them.

use std::fmt;
use std::sync::{Arc, LazyLock};

use regex::Regex;

/// Which file a line is of: the old one, whose lines a patch removes (its *deletions*), or the new
/// one, which it adds to (its *additions*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Deletions,
    Additions,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Deletions => Side::Additions,
            Side::Additions => Side::Deletions,
        }
    }
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeType {
    Change,
    /// Moved, with the same contents.
    RenamePure,
    /// Moved, and changed.
    RenameChanged,
    New,
    Deleted,
}

impl ChangeType {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeType::Change => "change",
            ChangeType::RenamePure => "rename-pure",
            ChangeType::RenameChanged => "rename-changed",
            ChangeType::New => "new",
            ChangeType::Deleted => "deleted",
        }
    }
}

/// A side's lines, each with its line end (the last one may have none), kept as one text: a
/// patch of megabytes is hundreds of thousands of lines.
#[derive(Clone, PartialEq, Eq)]
pub struct Lines {
    text: String,
    /// Where each line starts in `text`, and where the last one ends.
    bounds: Vec<usize>,
}

impl Default for Lines {
    fn default() -> Self {
        Lines::new()
    }
}

impl Lines {
    pub fn new() -> Self {
        Lines {
            text: String::new(),
            bounds: vec![0],
        }
    }

    /// `contents` split into lines, each keeping its newline; none for empty contents.
    pub fn of(contents: &str) -> Self {
        let mut bounds = vec![0];
        let mut start = 0;
        while let Some(newline) = contents[start..].find('\n') {
            start += newline + 1;
            bounds.push(start);
        }
        if start < contents.len() {
            bounds.push(contents.len());
        }
        Lines {
            text: contents.to_owned(),
            bounds,
        }
    }

    pub fn len(&self) -> usize {
        self.bounds.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The line `ix` (from 0) with its line end.
    pub fn get(&self, ix: usize) -> Option<&str> {
        (ix < self.len()).then(|| &self.text[self.bounds[ix]..self.bounds[ix + 1]])
    }

    /// The line `ix`, which must be one.
    pub fn line(&self, ix: usize) -> &str {
        &self.text[self.bounds[ix]..self.bounds[ix + 1]]
    }

    /// Where the line `ix` is in [`text`](Self::text).
    pub fn range(&self, ix: usize) -> std::ops::Range<usize> {
        self.bounds[ix]..self.bounds[ix + 1]
    }

    /// All the lines, one after the other.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn push(&mut self, line: &str) {
        self.text.push_str(line);
        self.bounds.push(self.text.len());
    }

    /// Takes the line end off the last line, if it has one (`cleanLastNewline`).
    fn trim_last_newline(&mut self) {
        let Some(&end) = self.bounds.last() else {
            return;
        };
        if self.is_empty() {
            return;
        }
        let start = self.bounds[self.len() - 1];
        let line = &self.text[start..end];
        let trimmed = line
            .strip_suffix('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .unwrap_or(line)
            .len();
        self.text.truncate(start + trimmed);
        *self.bounds.last_mut().expect("a line") = self.text.len();
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> + '_ {
        (0..self.len()).map(|ix| self.line(ix))
    }
}

impl fmt::Debug for Lines {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// A run of a hunk's lines: unchanged ones, or changed ones (the removed lines, then the added
/// ones, as git writes them). The indexes are into the diff's [`Lines`] of each side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkContent {
    Context {
        lines: usize,
        deletion_line_index: usize,
        addition_line_index: usize,
    },
    Change {
        deletions: usize,
        deletion_line_index: usize,
        additions: usize,
        addition_line_index: usize,
    },
}

/// One `@@ -a,b +c,d @@` block of a patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// Unchanged lines between the previous hunk (or the top of the file) and this one.
    pub collapsed_before: usize,
    /// The first line of the new file the hunk covers, and how many (unchanged ones included).
    pub addition_start: usize,
    pub addition_count: usize,
    /// How many lines it adds.
    pub addition_lines: usize,
    /// Where its lines start in the diff's additions.
    pub addition_line_index: usize,
    pub deletion_start: usize,
    pub deletion_count: usize,
    pub deletion_lines: usize,
    pub deletion_line_index: usize,
    pub content: Vec<HunkContent>,
    /// What git says the lines are in, after the `@@`, like a function's name.
    pub context: Option<String>,
    /// The `@@` line.
    pub specs: String,
    /// Rows the hunk takes side by side, and in one column, and where they start, counting the
    /// unchanged lines before each hunk (`splitLineStart`, `unifiedLineStart`).
    pub split_line_start: usize,
    pub split_line_count: usize,
    pub unified_line_start: usize,
    pub unified_line_count: usize,
    /// Whether the last line of either side has no line end.
    pub no_eof_newline_deletions: bool,
    pub no_eof_newline_additions: bool,
}

/// One file of a patch, parsed (`FileDiffMetadata`). Cheap to clone: the sides are shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub name: String,
    /// The name it had, if it was moved.
    pub prev_name: Option<String>,
    pub kind: ChangeType,
    pub hunks: Vec<Hunk>,
    pub deletion_lines: Arc<Lines>,
    pub addition_lines: Arc<Lines>,
    /// The object names on the patch's `index` line, to read both files whole by.
    pub prev_object_id: Option<String>,
    pub new_object_id: Option<String>,
    pub mode: Option<String>,
    pub prev_mode: Option<String>,
    /// Whether the sides only have the patch's lines, rather than the whole files.
    pub is_partial: bool,
    /// Names the diff in the highlighting cache.
    pub cache_key: Option<String>,
}

impl FileDiff {
    pub fn lines(&self, side: Side) -> &Lines {
        match side {
            Side::Deletions => &self.deletion_lines,
            Side::Additions => &self.addition_lines,
        }
    }

    /// Where the line `line_number` of `side` is among the rows, in one column and side by side
    /// (`getLineIndexForDiff`): counting the unchanged lines between hunks as rows, which is
    /// enough to order lines by.
    pub fn line_index(&self, line_number: usize, side: Side) -> Option<(usize, usize)> {
        let last = self.hunks.len().checked_sub(1)?;
        for (ix, hunk) in self.hunks.iter().enumerate() {
            let (start, count) = match side {
                Side::Deletions => (hunk.deletion_start, hunk.deletion_count),
                Side::Additions => (hunk.addition_start, hunk.addition_count),
            };
            let mut current = start_boundary(start, count) + 1;
            let mut split = hunk.split_line_start;
            let mut unified = hunk.unified_line_start;
            if line_number < current {
                let difference = current - line_number;
                return Some((
                    unified.saturating_sub(difference),
                    split.saturating_sub(difference),
                ));
            }
            if line_number >= current + count {
                if ix == last {
                    let difference = line_number - (current + count);
                    return Some((
                        unified + hunk.unified_line_count + difference,
                        split + hunk.split_line_count + difference,
                    ));
                }
                continue;
            }
            for content in &hunk.content {
                match *content {
                    HunkContent::Context { lines, .. } => {
                        if line_number < current + lines {
                            let difference = line_number - current;
                            return Some((unified + difference, split + difference));
                        }
                        current += lines;
                        split += lines;
                        unified += lines;
                    }
                    HunkContent::Change {
                        deletions,
                        additions,
                        ..
                    } => {
                        let count = match side {
                            Side::Deletions => deletions,
                            Side::Additions => additions,
                        };
                        if line_number < current + count {
                            let difference = line_number - current;
                            let before = if side == Side::Additions {
                                deletions
                            } else {
                                0
                            };
                            return Some((unified + before + difference, split + difference));
                        }
                        current += count;
                        split += deletions.max(additions);
                        unified += deletions + additions;
                    }
                }
            }
            return None;
        }
        None
    }
}

/// Where a hunk's lines start in one of its files, from 0: one with no lines there starts after
/// `start` (`getHunkSideStartBoundary`).
pub fn start_boundary(start: usize, count: usize) -> usize {
    if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    }
}

/// Where a hunk's lines end in one of its files, from 0 (`getHunkSideEndBoundary`).
pub fn end_boundary(start: usize, count: usize) -> usize {
    start_boundary(start, count) + count
}

/// A file read whole, to fill in a diff with, named in the highlighting cache.
#[derive(Debug, Clone, Copy)]
pub struct LoadedSide<'a> {
    pub contents: &'a str,
    pub cache_key: Option<&'a str>,
}

/// Parses each file of a patch (`parsePatchFiles`): partial diffs, with the patch's lines. Files
/// are named in the highlighting cache by `cache_key` and their place in the patch.
pub fn parse_patch_files(patch: &str, cache_key: Option<&str>) -> Vec<FileDiff> {
    let is_git = patch.starts_with("diff --git") || patch.contains("\ndiff --git");
    let parts = if is_git {
        split_at_line_prefix(patch, "diff --git")
    } else {
        vec![patch]
    };
    let mut files = Vec::new();
    for part in parts {
        if (is_git && !part.starts_with("diff --git"))
            || (!is_git && !starts_with_unified_header(part))
        {
            // What's before the first file, like a commit's message.
            continue;
        }
        let key = cache_key.map(|key| format!("{key}:{}", files.len()));
        if let Some(file) = process_file(part, key, is_git, None) {
            files.push(file);
        }
    }
    files
}

/// A diff from `old` to `new`, both whole (`parseDiffFromFile`), with three lines around each
/// change, as git would give.
pub fn diff_from_files(
    old_name: &str,
    old: LoadedSide<'_>,
    new_name: &str,
    new: LoadedSide<'_>,
) -> FileDiff {
    let patch = similar::TextDiff::from_lines(old.contents, new.contents)
        .unified_diff()
        .context_radius(3)
        .header(old_name, new_name)
        .to_string();
    let cache_key = match (old.cache_key, new.cache_key) {
        (Some(old), Some(new)) => Some(format!("{old}:{new}")),
        _ => None,
    };
    process_file(&patch, cache_key, false, Some((old.contents, new.contents))).unwrap_or_else(
        || FileDiff {
            name: new_name.to_owned(),
            prev_name: None,
            kind: ChangeType::Change,
            hunks: Vec::new(),
            deletion_lines: Arc::new(Lines::of(old.contents)),
            addition_lines: Arc::new(Lines::of(new.contents)),
            prev_object_id: None,
            new_object_id: None,
            mode: None,
            prev_mode: None,
            is_partial: false,
            cache_key: None,
        },
    )
}

/// `diff`, a partial one, filled in with both files whole (`hydratePartialDiff`): its sides are
/// the files' lines, and its hunks index into them. Only a change, or a move, can be: an added or
/// deleted file's patch has all of it already.
pub fn hydrate(
    diff: &FileDiff,
    old: Option<LoadedSide<'_>>,
    new: Option<LoadedSide<'_>>,
) -> Result<FileDiff, String> {
    if !diff.is_partial {
        return Err("hydratePartialDiff: fileDiff must be partial".to_owned());
    }
    let mut hydrated = diff.clone();
    match diff.kind {
        ChangeType::Change | ChangeType::RenameChanged => {
            let old = required(diff, old, "oldFile")?;
            let new = required(diff, new, "newFile")?;
            let deletions = Lines::of(old.contents);
            let additions = Lines::of(new.contents);
            hydrated.hunks = hydrate_hunks(&diff.hunks);
            hydrated.deletion_lines = Arc::new(deletions);
            hydrated.addition_lines = Arc::new(additions);
            hydrated.is_partial = false;
            hydrated.cache_key = hydrated_cache_key(diff, Some(old), Some(new));
            Ok(hydrated)
        }
        ChangeType::RenamePure => {
            let new = required(diff, new, "newFile")?;
            if old.is_some() {
                return Err(format!(
                    "hydratePartialDiff: {} diff for {} requires oldFile to be null",
                    diff.kind.as_str(),
                    diff.name
                ));
            }
            let lines = Arc::new(Lines::of(new.contents));
            hydrated.deletion_lines = Arc::clone(&lines);
            hydrated.addition_lines = lines;
            hydrated.is_partial = false;
            hydrated.cache_key = hydrated_cache_key(diff, None, Some(new));
            Ok(hydrated)
        }
        ChangeType::New | ChangeType::Deleted => Err(format!(
            "hydratePartialDiff: {} diffs cannot be hydrated from loaded files",
            diff.kind.as_str()
        )),
    }
}

/// `file`, which hydrating `diff` needs.
fn required<'a>(
    diff: &FileDiff,
    file: Option<LoadedSide<'a>>,
    which: &str,
) -> Result<LoadedSide<'a>, String> {
    file.ok_or_else(|| {
        format!(
            "hydratePartialDiff: {} diff for {} requires {which}",
            diff.kind.as_str(),
            diff.name
        )
    })
}

fn hydrated_cache_key(
    diff: &FileDiff,
    old: Option<LoadedSide<'_>>,
    new: Option<LoadedSide<'_>>,
) -> Option<String> {
    if let Some(key) = &diff.cache_key {
        return Some(format!("{key}:hydrated"));
    }
    match (old, new) {
        (Some(old), Some(new)) => match (old.cache_key, new.cache_key) {
            (Some(old), Some(new)) => Some(format!("hydrated-files:{old}:{new}")),
            _ => None,
        },
        (old, new) => old
            .or(new)
            .and_then(|file| file.cache_key.map(str::to_owned)),
    }
}

/// The hunks of a partial diff, indexing into the whole files instead.
fn hydrate_hunks(hunks: &[Hunk]) -> Vec<Hunk> {
    let mut split_count = 0;
    let mut unified_count = 0;
    let mut last_end = 0;
    hunks
        .iter()
        .map(|hunk| {
            let addition_line_index = hunk.addition_start.saturating_sub(1);
            let deletion_line_index = hunk.deletion_start.saturating_sub(1);
            let mut addition_ix = addition_line_index;
            let mut deletion_ix = deletion_line_index;
            let content = hunk
                .content
                .iter()
                .map(|content| match *content {
                    HunkContent::Context { lines, .. } => {
                        let hydrated = HunkContent::Context {
                            lines,
                            deletion_line_index: deletion_ix,
                            addition_line_index: addition_ix,
                        };
                        addition_ix += lines;
                        deletion_ix += lines;
                        hydrated
                    }
                    HunkContent::Change {
                        deletions,
                        additions,
                        ..
                    } => {
                        let hydrated = HunkContent::Change {
                            deletions,
                            deletion_line_index: deletion_ix,
                            additions,
                            addition_line_index: addition_ix,
                        };
                        addition_ix += additions;
                        deletion_ix += deletions;
                        hydrated
                    }
                })
                .collect();
            let collapsed_before =
                start_boundary(hunk.addition_start, hunk.addition_count).saturating_sub(last_end);
            let hydrated = Hunk {
                collapsed_before,
                addition_line_index,
                deletion_line_index,
                content,
                split_line_start: split_count + collapsed_before,
                unified_line_start: unified_count + collapsed_before,
                ..hunk.clone()
            };
            split_count += collapsed_before + hunk.split_line_count;
            unified_count += collapsed_before + hunk.unified_line_count;
            last_end = end_boundary(hunk.addition_start, hunk.addition_count);
            hydrated
        })
        .collect()
}

/// The numbers of a hunk's `@@ -a,b +c,d @@ context` line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HunkHeader {
    deletion_start: usize,
    deletion_count: usize,
    addition_start: usize,
    addition_count: usize,
    context: Option<String>,
}

fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    let rest = line.strip_prefix("@@ -")?;
    let (deletion_start, rest) = read_number(rest)?;
    let (deletion_count, rest) = match rest.strip_prefix(',') {
        Some(rest) => read_number(rest)?,
        None => (1, rest),
    };
    let rest = rest.strip_prefix(" +")?;
    let (addition_start, rest) = read_number(rest)?;
    let (addition_count, rest) = match rest.strip_prefix(',') {
        Some(rest) => read_number(rest)?,
        None => (1, rest),
    };
    let rest = rest.strip_prefix(" @@")?;
    let context = rest.strip_prefix(' ').map(|context| {
        context
            .strip_suffix("\r\n")
            .or_else(|| context.strip_suffix('\n'))
            .unwrap_or(context)
            .to_owned()
    });
    Some(HunkHeader {
        deletion_start,
        deletion_count,
        addition_start,
        addition_count,
        context,
    })
}

fn read_number(text: &str) -> Option<(usize, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let value = text[..digits].parse().ok()?;
    Some((value, &text[digits..]))
}

/// `contents` split before each line that starts with `prefix`; what's before the first is a part
/// of its own.
fn split_at_line_prefix<'a>(contents: &'a str, prefix: &str) -> Vec<&'a str> {
    if contents.is_empty() {
        return vec![""];
    }
    let newline_prefix = format!("\n{prefix}");
    let find = |from: usize| {
        contents[from..]
            .find(&newline_prefix)
            .map(|ix| from + ix + 1)
    };
    let first = if contents.starts_with(prefix) {
        Some(0)
    } else {
        find(0)
    };
    let Some(first) = first else {
        return vec![contents];
    };
    let mut parts = Vec::new();
    if first > 0 {
        parts.push(&contents[..first]);
    }
    let mut start = first;
    while let Some(next) = find(start + 1) {
        parts.push(&contents[start..next]);
        start = next;
    }
    parts.push(&contents[start..]);
    parts
}

/// `contents` split into lines, each keeping its newline.
fn split_with_newlines(contents: &str) -> Vec<&str> {
    if contents.is_empty() {
        return vec![""];
    }
    contents.split_inclusive('\n').collect()
}

static GIT_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^diff --git ("a/(?:[^"\\]|\\.)*"|a/.+?) ("b/(?:[^"\\]|\\.)*"|b/.+?)$"#)
        .expect("a valid pattern")
});
static FILENAME_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(---|\+\+\+)\s+([^\t\r\n]+)").expect("a valid pattern"));
static INDEX_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^index ([0-9a-f]+)\.\.([0-9a-f]+)(?: (\d+))?$").expect("a valid pattern")
});

/// A file name as a patch has it: unquoted (git quotes names with unusual characters, C-style),
/// without git's `a/` or `b/` if `strip_git_prefix`.
fn decode_file_name(value: &str, strip_git_prefix: bool) -> String {
    let raw = value.trim();
    let name = if raw.starts_with('"') {
        unquote(raw).unwrap_or_else(|| raw.to_owned())
    } else {
        raw.to_owned()
    };
    if strip_git_prefix && (name.starts_with("a/") || name.starts_with("b/")) {
        name[2..].to_owned()
    } else {
        name
    }
}

/// A C-style quoted string's contents, its escapes undone; octal escapes are bytes of UTF-8.
fn unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let mut bytes = Vec::with_capacity(inner.len());
    let mut chars = inner.bytes().peekable();
    while let Some(byte) = chars.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        match chars.next()? {
            b'n' => bytes.push(b'\n'),
            b't' => bytes.push(b'\t'),
            b'r' => bytes.push(b'\r'),
            b'a' => bytes.push(7),
            b'b' => bytes.push(8),
            b'f' => bytes.push(12),
            b'v' => bytes.push(11),
            digit @ b'0'..=b'7' => {
                let mut value = u32::from(digit - b'0');
                for _ in 0..2 {
                    match chars.peek() {
                        Some(next @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(next - b'0');
                            chars.next();
                        }
                        _ => break,
                    }
                }
                bytes.push(u8::try_from(value).ok()?);
            }
            other => bytes.push(other),
        }
    }
    String::from_utf8(bytes).ok()
}

/// What a line of a hunk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineType {
    Context,
    Addition,
    Deletion,
    /// `\ No newline at end of file`.
    Metadata,
}

/// Parses one file's part of a patch (`processFile`): partial, or filled in with `files`, both
/// whole, when they're given.
fn process_file(
    text: &str,
    cache_key: Option<String>,
    is_git: bool,
    files: Option<(&str, &str)>,
) -> Option<FileDiff> {
    let is_partial = files.is_none();
    let mut current: Option<FileDiff> = None;
    let mut deletions = Lines::new();
    let mut additions = Lines::new();
    let mut last_hunk_end = 0;
    let mut split_count = 0;
    let mut unified_count = 0;
    let mut deletion_line_index = 0;
    let mut addition_line_index = 0;

    for chunk in split_at_line_prefix(text, "@@ ") {
        let mut lines = split_with_newlines(chunk);
        let first = lines[0];
        let header = parse_hunk_header(first);
        let (Some(header), Some(file)) = (header, current.as_mut()) else {
            if current.is_some() {
                // An invalid hunk: skipped, as the library does.
                continue;
            }
            let mut file = FileDiff {
                name: String::new(),
                prev_name: None,
                kind: ChangeType::Change,
                hunks: Vec::new(),
                deletion_lines: Arc::default(),
                addition_lines: Arc::default(),
                prev_object_id: None,
                new_object_id: None,
                mode: None,
                prev_mode: None,
                is_partial,
                cache_key: cache_key.clone(),
            };
            if let Some((old, new)) = files {
                deletions = Lines::of(old);
                additions = Lines::of(new);
            }
            for line in &lines {
                read_header_line(&mut file, line, is_git);
            }
            current = Some(file);
            continue;
        };

        while lines
            .last()
            .is_some_and(|line| matches!(*line, "\n" | "\r" | "\r\n" | ""))
        {
            lines.pop();
        }
        if !is_partial {
            deletion_line_index = header.deletion_start.saturating_sub(1);
            addition_line_index = header.addition_start.saturating_sub(1);
        }
        let mut hunk = Hunk {
            collapsed_before: 0,
            addition_start: header.addition_start,
            addition_count: header.addition_count,
            addition_lines: 0,
            addition_line_index,
            deletion_start: header.deletion_start,
            deletion_count: header.deletion_count,
            deletion_lines: 0,
            deletion_line_index,
            content: Vec::new(),
            context: header.context,
            specs: first.to_owned(),
            split_line_start: 0,
            split_line_count: 0,
            unified_line_start: 0,
            unified_line_count: 0,
            no_eof_newline_deletions: false,
            no_eof_newline_additions: false,
        };
        let mut parsed_additions = 0;
        let mut parsed_deletions = 0;
        let mut last_type = None;
        for raw in lines.iter().skip(1) {
            if parsed_additions >= hunk.addition_count
                && parsed_deletions >= hunk.deletion_count
                && !raw.starts_with('\\')
                && (!is_hunk_body_line(raw) || is_format_patch_separator(raw))
            {
                break;
            }
            let line_type = match raw.as_bytes().first() {
                Some(b' ') => LineType::Context,
                Some(b'+') => LineType::Addition,
                Some(b'-') => LineType::Deletion,
                Some(b'\\') => LineType::Metadata,
                _ => continue,
            };
            let content = if raw.len() > 1 { &raw[1..] } else { "\n" };
            match line_type {
                LineType::Addition | LineType::Deletion => {
                    if !matches!(hunk.content.last(), Some(HunkContent::Change { .. })) {
                        hunk.content.push(HunkContent::Change {
                            deletions: 0,
                            deletion_line_index,
                            additions: 0,
                            addition_line_index,
                        });
                    }
                    let Some(HunkContent::Change {
                        deletions: deleted,
                        additions: added,
                        ..
                    }) = hunk.content.last_mut()
                    else {
                        unreachable!("a change was pushed");
                    };
                    if line_type == LineType::Addition {
                        addition_line_index += 1;
                        parsed_additions += 1;
                        *added += 1;
                        hunk.addition_lines += 1;
                        if is_partial {
                            additions.push(content);
                        }
                    } else {
                        deletion_line_index += 1;
                        parsed_deletions += 1;
                        *deleted += 1;
                        hunk.deletion_lines += 1;
                        if is_partial {
                            deletions.push(content);
                        }
                    }
                    last_type = Some(line_type);
                }
                LineType::Context => {
                    if !matches!(hunk.content.last(), Some(HunkContent::Context { .. })) {
                        hunk.content.push(HunkContent::Context {
                            lines: 0,
                            deletion_line_index,
                            addition_line_index,
                        });
                    }
                    if let Some(HunkContent::Context { lines, .. }) = hunk.content.last_mut() {
                        *lines += 1;
                    }
                    addition_line_index += 1;
                    deletion_line_index += 1;
                    parsed_additions += 1;
                    parsed_deletions += 1;
                    if is_partial {
                        deletions.push(content);
                        additions.push(content);
                    }
                    last_type = Some(line_type);
                }
                LineType::Metadata => {
                    let Some(last_content) = hunk.content.last() else {
                        continue;
                    };
                    if matches!(last_content, HunkContent::Context { .. }) {
                        hunk.no_eof_newline_additions = true;
                        hunk.no_eof_newline_deletions = true;
                    } else if last_type == Some(LineType::Deletion) {
                        hunk.no_eof_newline_deletions = true;
                    } else if last_type == Some(LineType::Addition) {
                        hunk.no_eof_newline_additions = true;
                    }
                    if is_partial
                        && matches!(last_type, Some(LineType::Addition | LineType::Context))
                    {
                        additions.trim_last_newline();
                    }
                    if is_partial
                        && matches!(last_type, Some(LineType::Deletion | LineType::Context))
                    {
                        deletions.trim_last_newline();
                    }
                }
            }
        }
        if parsed_additions != hunk.addition_count || parsed_deletions != hunk.deletion_count {
            // A hunk with other lines than its header says: its numbers are taken from its lines.
            let addition_start = start_boundary(hunk.addition_start, hunk.addition_count)
                + usize::from(parsed_additions != 0);
            let deletion_start = start_boundary(hunk.deletion_start, hunk.deletion_count)
                + usize::from(parsed_deletions != 0);
            if !is_partial {
                let addition_delta = addition_start as isize - hunk.addition_start as isize;
                let deletion_delta = deletion_start as isize - hunk.deletion_start as isize;
                hunk.addition_line_index = hunk
                    .addition_line_index
                    .saturating_add_signed(addition_delta);
                hunk.deletion_line_index = hunk
                    .deletion_line_index
                    .saturating_add_signed(deletion_delta);
                for content in &mut hunk.content {
                    let (HunkContent::Context {
                        deletion_line_index,
                        addition_line_index,
                        ..
                    }
                    | HunkContent::Change {
                        deletion_line_index,
                        addition_line_index,
                        ..
                    }) = content;
                    *addition_line_index =
                        addition_line_index.saturating_add_signed(addition_delta);
                    *deletion_line_index =
                        deletion_line_index.saturating_add_signed(deletion_delta);
                }
            }
            hunk.addition_start = addition_start;
            hunk.deletion_start = deletion_start;
            hunk.addition_count = parsed_additions;
            hunk.deletion_count = parsed_deletions;
        }
        hunk.collapsed_before =
            start_boundary(hunk.addition_start, hunk.addition_count).saturating_sub(last_hunk_end);
        last_hunk_end = end_boundary(hunk.addition_start, hunk.addition_count);
        for content in &hunk.content {
            match *content {
                HunkContent::Context { lines, .. } => {
                    hunk.split_line_count += lines;
                    hunk.unified_line_count += lines;
                }
                HunkContent::Change {
                    deletions,
                    additions,
                    ..
                } => {
                    hunk.split_line_count += deletions.max(additions);
                    hunk.unified_line_count += deletions + additions;
                }
            }
        }
        hunk.split_line_start = split_count + hunk.collapsed_before;
        hunk.unified_line_start = unified_count + hunk.collapsed_before;
        split_count += hunk.collapsed_before + hunk.split_line_count;
        unified_count += hunk.collapsed_before + hunk.unified_line_count;
        file.hunks.push(hunk);
    }

    let mut file = current?;
    file.deletion_lines = Arc::new(deletions);
    file.addition_lines = Arc::new(additions);
    if !is_git {
        let (old, new) = files.unwrap_or_default();
        if file.prev_name.is_some() && file.prev_name.as_deref() != Some(file.name.as_str()) {
            file.kind = if file.hunks.is_empty() {
                ChangeType::RenamePure
            } else {
                ChangeType::RenameChanged
            };
        } else if files.is_some() && old.is_empty() && !new.is_empty() {
            file.kind = ChangeType::New;
        } else if files.is_some() && !old.is_empty() && new.is_empty() {
            file.kind = ChangeType::Deleted;
        }
    }
    if !matches!(
        file.kind,
        ChangeType::RenamePure | ChangeType::RenameChanged
    ) {
        file.prev_name = None;
    }
    Some(file)
}

/// Reads what a line of a file's header says of it: its names, modes, object names, and how it
/// changed.
fn read_header_line(file: &mut FileDiff, line: &str, is_git: bool) {
    if line.starts_with("diff --git") {
        if let Some(names) = GIT_HEADER.captures(line.trim()) {
            let prev_name = decode_file_name(&names[1], true);
            let name = decode_file_name(&names[2], true);
            if prev_name != name {
                file.prev_name = Some(prev_name);
            }
            file.name = name;
        }
        return;
    }
    if (line.starts_with("---") || line.starts_with("+++"))
        && let Some(header) = FILENAME_HEADER.captures(line)
    {
        let name = decode_file_name(&header[2], is_git);
        if &header[1] == "---" && name != "/dev/null" {
            file.prev_name = Some(name.clone());
            file.name = name;
        } else if &header[1] == "+++" && name != "/dev/null" {
            file.name = name;
        }
        return;
    }
    if !is_git {
        return;
    }
    let line = line.trim_end_matches(['\n', '\r']);
    if let Some(mode) = line.strip_prefix("new mode ") {
        file.mode = Some(mode.trim().to_owned());
    }
    if let Some(mode) = line.strip_prefix("old mode ") {
        file.prev_mode = Some(mode.trim().to_owned());
    }
    if let Some(mode) = line.strip_prefix("new file mode") {
        file.kind = ChangeType::New;
        file.mode = Some(mode.trim().to_owned());
    }
    if let Some(mode) = line.strip_prefix("deleted file mode") {
        file.kind = ChangeType::Deleted;
        file.mode = Some(mode.trim().to_owned());
    }
    if line.starts_with("similarity index") {
        file.kind = if line.starts_with("similarity index 100%") {
            ChangeType::RenamePure
        } else {
            ChangeType::RenameChanged
        };
    }
    if line.starts_with("index ")
        && let Some(index) = INDEX_LINE.captures(line.trim())
    {
        file.prev_object_id = Some(index[1].to_owned());
        file.new_object_id = Some(index[2].to_owned());
        if let Some(mode) = index.get(3) {
            file.mode = Some(mode.as_str().to_owned());
        }
    }
    if let Some(name) = line
        .strip_prefix("rename from ")
        .or_else(|| line.strip_prefix("copy from "))
    {
        file.prev_name = Some(decode_file_name(name, false));
    }
    if let Some(name) = line
        .strip_prefix("rename to ")
        .or_else(|| line.strip_prefix("copy to "))
    {
        file.name = decode_file_name(name, false);
    }
}

/// Whether `contents` start with a file's `---` and `+++` lines, as a patch that isn't git's does.
fn starts_with_unified_header(contents: &str) -> bool {
    let mut lines = contents.lines();
    let named = |line: Option<&str>, prefix: &str| {
        line.and_then(|line| line.strip_prefix(prefix))
            .is_some_and(|rest| rest.starts_with([' ', '\t']) && !rest.trim().is_empty())
    };
    named(lines.next(), "---") && named(lines.next(), "+++")
}

fn is_hunk_body_line(line: &str) -> bool {
    matches!(line.as_bytes().first(), Some(b'+' | b'-' | b' '))
}

/// `-- ` on its own, which ends a patch `git format-patch` made.
fn is_format_patch_separator(line: &str) -> bool {
    line.strip_prefix("--")
        .is_some_and(|rest| rest.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(patch: &str) -> FileDiff {
        parse_patch_files(patch, Some("test")).remove(0)
    }

    #[test]
    fn parses_a_files_hunks_and_lines() {
        let diff = parse(
            "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@ fn main\n one\n-two\n+TWO\n three\n",
        );
        assert_eq!(diff.name, "f.txt");
        assert_eq!(diff.kind, ChangeType::Change);
        assert_eq!(diff.prev_object_id.as_deref(), Some("1111111"));
        assert_eq!(diff.new_object_id.as_deref(), Some("2222222"));
        assert_eq!(
            diff.deletion_lines.iter().collect::<Vec<_>>(),
            ["one\n", "two\n", "three\n"]
        );
        assert_eq!(
            diff.addition_lines.iter().collect::<Vec<_>>(),
            ["one\n", "TWO\n", "three\n"]
        );
        let hunk = &diff.hunks[0];
        assert_eq!(hunk.context.as_deref(), Some("fn main"));
        assert_eq!(
            hunk.content,
            [
                HunkContent::Context {
                    lines: 1,
                    deletion_line_index: 0,
                    addition_line_index: 0
                },
                HunkContent::Change {
                    deletions: 1,
                    deletion_line_index: 1,
                    additions: 1,
                    addition_line_index: 1
                },
                HunkContent::Context {
                    lines: 1,
                    deletion_line_index: 2,
                    addition_line_index: 2
                },
            ]
        );
    }

    #[test]
    fn counts_the_lines_hidden_between_hunks() {
        let diff = parse(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -5,2 +5,2 @@\n-a\n+b\n c\n@@ -20,1 +20,1 @@\n-x\n+y\n",
        );
        assert_eq!(diff.hunks[0].collapsed_before, 4);
        assert_eq!(diff.hunks[1].collapsed_before, 13);
    }

    #[test]
    fn keeps_a_line_without_its_newline_as_the_patch_says() {
        let diff = parse(
            "diff --git a/link b/link\nindex 1..2 120000\n--- a/link\n+++ b/link\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n",
        );
        assert_eq!(diff.deletion_lines.iter().collect::<Vec<_>>(), ["a"]);
        assert_eq!(diff.addition_lines.iter().collect::<Vec<_>>(), ["b"]);
        assert!(diff.hunks[0].no_eof_newline_deletions);
        assert!(diff.hunks[0].no_eof_newline_additions);
    }

    #[test]
    fn reads_renames_and_quoted_names() {
        let diff = parse(
            "diff --git a/old name.txt b/new name.txt\nsimilarity index 90%\nrename from old name.txt\nrename to new name.txt\n--- a/old name.txt\n+++ b/new name.txt\n@@ -1 +1 @@\n-a\n+b\n",
        );
        assert_eq!(diff.kind, ChangeType::RenameChanged);
        assert_eq!(diff.prev_name.as_deref(), Some("old name.txt"));
        assert_eq!(diff.name, "new name.txt");

        let quoted = parse(
            "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\nnew file mode 100644\n--- /dev/null\n+++ \"b/caf\\303\\251.txt\"\n@@ -0,0 +1 @@\n+a\n",
        );
        assert_eq!(quoted.name, "café.txt");
        assert_eq!(quoted.kind, ChangeType::New);
        assert_eq!(quoted.prev_name, None);
    }

    #[test]
    fn fills_a_patch_in_with_the_whole_files() {
        let diff = parse(
            "diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -2,1 +2,1 @@\n-b\n+B\n",
        );
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let hydrated = hydrate(
            &diff,
            Some(LoadedSide {
                contents: old,
                cache_key: Some("1"),
            }),
            Some(LoadedSide {
                contents: new,
                cache_key: Some("2"),
            }),
        )
        .unwrap();
        assert!(!hydrated.is_partial);
        assert_eq!(hydrated.deletion_lines.len(), 3);
        assert_eq!(hydrated.hunks[0].deletion_line_index, 1);
        assert_eq!(hydrated.cache_key.as_deref(), Some("test:0:hydrated"));
        assert!(hydrate(&hydrated, None, None).is_err());
    }

    #[test]
    fn makes_a_diff_of_two_whole_files() {
        let diff = diff_from_files(
            "a.txt",
            LoadedSide {
                contents: "one\ntwo\n",
                cache_key: None,
            },
            "a.txt",
            LoadedSide {
                contents: "one\nthree\n",
                cache_key: None,
            },
        );
        assert!(!diff.is_partial);
        assert_eq!(diff.kind, ChangeType::Change);
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].deletion_lines, 1);
        assert_eq!(diff.hunks[0].addition_lines, 1);
    }
}
