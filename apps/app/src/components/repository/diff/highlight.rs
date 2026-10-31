//! What's highlighted in a diff's lines: their syntax, and the words that changed between a
//! removed line and the line added in its place. Worked out off the main thread, once per diff
//! (see `viewer_runtime`'s cache), and styled with the theme as the lines are drawn, so it follows
//! the theme as it changes.
//!
//! Syntax is syntect's, with bat's syntax set, as gpui-kit's `syntect` feature has it (which keeps
//! its highlighter to itself): TextMate scopes are named by the theme's highlight names
//! (`keyword`, `string`…) the way gpui-kit names them, so code in a diff looks as it does anywhere
//! else in the app. The TS viewer had Shiki do this, with @pierre/diffs' `word-alt` changed words.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use gitto_ui::diff::diff_metadata::{FileDiff, HunkContent, Side};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// Loaded once, on the first highlight: deserializing it takes tens of milliseconds.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Lines (of either side) above which a diff isn't highlighted at all (`tokenizeMaxLength`).
pub const MAX_HIGHLIGHTED_LINES: usize = 100_000;

/// Bytes above which a line isn't highlighted (`tokenizeMaxLineLength`), nor compared for the
/// words that changed (`maxLineDiffLength`): a minified file's lines take long to, for nothing
/// anyone reads.
pub const MAX_HIGHLIGHTED_LINE: usize = 1000;

/// The theme's highlight names a line's spans are styled by, numbered.
const NAMES: &[&str] = &[
    "comment",
    "comment.doc",
    "string",
    "string.escape",
    "string.regex",
    "number",
    "boolean",
    "constant",
    "operator",
    "preproc",
    "keyword",
    "type",
    "function",
    "tag",
    "attribute",
    "label",
    "title",
    "variable.special",
    "property",
    "variable",
    "emphasis.strong",
    "emphasis",
    "link_uri",
    "text.literal",
    "punctuation.list_marker",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation",
];

/// The highlight name a span is styled by.
pub fn highlight_name(ix: u8) -> &'static str {
    NAMES[usize::from(ix)]
}

/// Byte ranges in each line of a side, from its start, with what they are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineSpans<T> {
    /// `starts[ix]..starts[ix + 1]` index the spans of line `ix`.
    starts: Vec<u32>,
    spans: Vec<(Range<u32>, T)>,
}

impl<T> LineSpans<T> {
    fn new() -> Self {
        LineSpans {
            starts: vec![0],
            spans: Vec::new(),
        }
    }

    /// Ends a line, with the spans pushed since the last.
    fn end_line(&mut self) {
        self.starts.push(self.spans.len() as u32);
    }

    fn push(&mut self, range: Range<u32>, value: T) {
        self.spans.push((range, value));
    }

    /// The spans of line `ix`; none for a line past the last one worked out.
    pub fn line(&self, ix: usize) -> &[(Range<u32>, T)] {
        match (self.starts.get(ix), self.starts.get(ix + 1)) {
            (Some(start), Some(end)) => &self.spans[*start as usize..*end as usize],
            _ => &[],
        }
    }
}

/// What's highlighted in each line of a diff's sides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffHighlights {
    /// By side (deletions, additions): highlight names, numbered (see [`highlight_name`]).
    syntax: [LineSpans<u8>; 2],
    /// By side: the words that changed, in a line paired with one on the other side.
    changed: [LineSpans<()>; 2],
}

impl DiffHighlights {
    /// Works out what's highlighted in `diff`. Blocking: run it on a background thread.
    pub fn compute(diff: &FileDiff) -> Self {
        let too_long =
            diff.deletion_lines.len().max(diff.addition_lines.len()) > MAX_HIGHLIGHTED_LINES;
        let syntax = [Side::Deletions, Side::Additions].map(|side| {
            let name = match side {
                Side::Deletions => diff.prev_name.as_deref().unwrap_or(&diff.name),
                Side::Additions => &diff.name,
            };
            match find_syntax(name).filter(|_| !too_long) {
                Some(syntax) => syntax_spans(diff, side, syntax),
                None => LineSpans::new(),
            }
        });
        DiffHighlights {
            syntax,
            changed: changed_words(diff),
        }
    }

    /// The syntax of line `ix` of `side`: byte ranges, and the highlight names they're styled by.
    pub fn syntax(&self, side: Side, ix: usize) -> &[(Range<u32>, u8)] {
        self.syntax[side_ix(side)].line(ix)
    }

    /// The words that changed in line `ix` of `side`.
    pub fn changed(&self, side: Side, ix: usize) -> &[(Range<u32>, ())] {
        self.changed[side_ix(side)].line(ix)
    }
}

fn side_ix(side: Side) -> usize {
    match side {
        Side::Deletions => 0,
        Side::Additions => 1,
    }
}

/// The syntax of a file by its path: by its whole name (`Makefile`), or its extension.
fn find_syntax(path: &str) -> Option<&'static SyntaxReference> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let syntaxes = &*SYNTAXES;
    syntaxes
        .find_syntax_by_extension(name)
        .or_else(|| {
            name.rsplit_once('.')
                .and_then(|(_, extension)| syntaxes.find_syntax_by_extension(extension))
        })
        .filter(|syntax| syntax.name != "Plain Text")
}

/// The syntax of each line of `side`. A partial diff's hunks are parsed each on their own, so an
/// unterminated comment or string in one can't color the next; a whole file is parsed whole.
fn syntax_spans(diff: &FileDiff, side: Side, syntax: &SyntaxReference) -> LineSpans<u8> {
    let lines = diff.lines(side);
    let mut spans = LineSpans::new();
    let mut names = HashMap::new();
    let mut parse = |range: Range<usize>, spans: &mut LineSpans<u8>| {
        let mut parser = ParseState::new(syntax);
        let mut scopes = ScopeStack::new();
        for ix in range {
            parse_line(lines.line(ix), &mut parser, &mut scopes, &mut names, spans);
            spans.end_line();
        }
    };
    if diff.is_partial {
        let mut next = 0;
        for hunk in &diff.hunks {
            let (start, count) = match side {
                Side::Deletions => (hunk.deletion_line_index, hunk.deletion_count),
                Side::Additions => (hunk.addition_line_index, hunk.addition_count),
            };
            let start = start.min(lines.len());
            let end = (start + count).min(lines.len());
            // Lines no hunk has (none, in a patch git wrote) aren't highlighted.
            while next < start {
                spans.end_line();
                next += 1;
            }
            parse(start.max(next)..end, &mut spans);
            next = next.max(end);
        }
    } else {
        parse(0..lines.len(), &mut spans);
    }
    spans
}

/// Parses `line`, with its line end, pushing its spans; a line too long to highlight keeps the
/// parser where it was, as if it was empty.
fn parse_line(
    line: &str,
    parser: &mut ParseState,
    scopes: &mut ScopeStack,
    names: &mut HashMap<Scope, Option<u8>>,
    spans: &mut LineSpans<u8>,
) {
    let content = line.trim_end_matches(['\n', '\r']);
    if content.len() > MAX_HIGHLIGHTED_LINE {
        let _ = parser.parse_line("\n", &SYNTAXES);
        return;
    }
    // The syntax set wants each line with its newline.
    let owned;
    let text = if line.ends_with('\n') {
        line
    } else {
        owned = format!("{line}\n");
        &owned
    };
    let Ok(operations) = parser.parse_line(text, &SYNTAXES) else {
        return;
    };
    let mut push = |range: Range<usize>, scopes: &ScopeStack, spans: &mut LineSpans<u8>| {
        let range = range.start.min(content.len())..range.end.min(content.len());
        if range.is_empty() {
            return;
        }
        // The innermost scope that names something wins, so the quotes of a string color as
        // the string rather than as punctuation.
        let name = scopes
            .scopes
            .iter()
            .rev()
            .find_map(|scope| *names.entry(*scope).or_insert_with(|| semantic_name(*scope)));
        let Some(name) = name else {
            return;
        };
        let range = range.start as u32..range.end as u32;
        // Only joined to a span of the same line.
        let line_start = spans.starts.last().map_or(0, |start| *start as usize);
        let in_line = spans.spans.len() > line_start;
        match spans.spans.last_mut() {
            Some((last, last_name)) if in_line && last.end == range.start && *last_name == name => {
                last.end = range.end;
            }
            _ => spans.push(range, name),
        }
    };
    let mut cursor = 0;
    for (index, operation) in operations {
        push(cursor..index, scopes, spans);
        let _ = scopes.apply(&operation);
        cursor = index;
    }
    push(cursor..text.len(), scopes, spans);
}

/// The theme's highlight name for a TextMate scope, or `None` to let an enclosing scope decide:
/// gpui-kit's mapping (`syntect_backend::semantic_name`), so colors are the app's everywhere.
fn semantic_name(scope: Scope) -> Option<u8> {
    let scope = scope.build_string();
    let is = |prefix: &str| {
        scope == prefix
            || scope
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    let name = if is("punctuation.definition.string")
        || is("punctuation.definition.comment")
        || is("invalid")
        || is("meta")
        || is("source")
        || is("text")
    {
        // Delimiters belong to what they delimit; the rest are containers with no color.
        return None;
    } else if is("comment.block.documentation") || is("comment.line.documentation") {
        "comment.doc"
    } else if is("comment") {
        "comment"
    } else if is("constant.character.escape") {
        "string.escape"
    } else if is("string.regexp") {
        "string.regex"
    } else if is("string") {
        "string"
    } else if is("constant.numeric") {
        "number"
    } else if is("constant.language.boolean") {
        "boolean"
    } else if is("constant") || is("support.constant") || is("entity.name.constant") {
        "constant"
    } else if is("keyword.operator") {
        "operator"
    } else if is("keyword.control.import") {
        "preproc"
    } else if is("keyword") || is("storage.modifier") {
        "keyword"
    } else if is("storage.type") {
        // `fn`, `class` and `let` are declarations, `int` and `bool` are types.
        if [
            "class",
            "enum",
            "function",
            "impl",
            "interface",
            "module",
            "namespace",
            "struct",
            "trait",
            "type",
            "let",
            "var",
            "const",
        ]
        .iter()
        .any(|word| scope.contains(word))
        {
            "keyword"
        } else {
            "type"
        }
    } else if is("entity.name.function")
        || is("support.function")
        || is("variable.function")
        || is("meta.function-call")
    {
        "function"
    } else if is("entity.name.type")
        || is("entity.name.class")
        || is("entity.name.struct")
        || is("entity.name.enum")
        || is("entity.name.trait")
        || is("entity.name.interface")
        || is("entity.other.inherited-class")
        || is("support.type")
        || is("support.class")
    {
        "type"
    } else if is("entity.name.tag") {
        "tag"
    } else if is("entity.other.attribute-name") {
        "attribute"
    } else if is("entity.name.label") {
        "label"
    } else if is("entity.name.section") || is("markup.heading") {
        "title"
    } else if is("variable.language") {
        "variable.special"
    } else if is("variable.other.member")
        || is("variable.other.property")
        || is("support.variable.property")
        || is("meta.mapping.key")
    {
        "property"
    } else if is("variable") {
        "variable"
    } else if is("markup.bold") {
        "emphasis.strong"
    } else if is("markup.italic") {
        "emphasis"
    } else if is("markup.underline.link") {
        "link_uri"
    } else if is("markup.raw") {
        "text.literal"
    } else if is("markup.list") {
        "punctuation.list_marker"
    } else if is("punctuation.section") || is("punctuation.definition.block") {
        "punctuation.bracket"
    } else if is("punctuation.separator") || is("punctuation.terminator") {
        "punctuation.delimiter"
    } else if is("punctuation") {
        "punctuation"
    } else {
        return None;
    };
    NAMES
        .iter()
        .position(|known| *known == name)
        .map(|ix| ix as u8)
}

/// The words that changed between each removed line and the added line in its place: within a
/// run of changes, the first removed line is paired with the first added one, and so on.
fn changed_words(diff: &FileDiff) -> [LineSpans<()>; 2] {
    let mut sides: [Vec<Vec<Range<u32>>>; 2] = [
        vec![Vec::new(); diff.deletion_lines.len()],
        vec![Vec::new(); diff.addition_lines.len()],
    ];
    for hunk in &diff.hunks {
        for content in &hunk.content {
            let HunkContent::Change {
                deletions,
                deletion_line_index,
                additions,
                addition_line_index,
            } = *content
            else {
                continue;
            };
            for i in 0..deletions.min(additions) {
                let (old_ix, new_ix) = (deletion_line_index + i, addition_line_index + i);
                let (Some(old), Some(new)) = (
                    diff.deletion_lines.get(old_ix),
                    diff.addition_lines.get(new_ix),
                ) else {
                    continue;
                };
                let old = old.trim_end_matches(['\n', '\r']);
                let new = new.trim_end_matches(['\n', '\r']);
                if old.len() > MAX_HIGHLIGHTED_LINE
                    || new.len() > MAX_HIGHLIGHTED_LINE
                    || old == new
                {
                    continue;
                }
                let (old_runs, new_runs) = changed_runs(old, new);
                sides[0][old_ix] = old_runs;
                sides[1][new_ix] = new_runs;
            }
        }
    }
    sides.map(|lines| {
        let mut spans = LineSpans::new();
        for line in lines {
            for range in line {
                spans.push(range, ());
            }
            spans.end_line();
        }
        spans
    })
}

/// The byte ranges that differ between two lines, on each side, by word (as jsdiff's
/// `diffWordsWithSpace`, which the TS viewer's word diffs used: a word is a run of letters, digits
/// and `_`, and every other character but whitespace is a token of its own): changed words
/// separated only by whitespace are one run.
pub fn changed_runs(old: &str, new: &str) -> (Vec<Range<u32>>, Vec<Range<u32>>) {
    let (old_tokens, new_tokens) = (tokens(old), tokens(new));
    let ops = similar::capture_diff_slices(similar::Algorithm::Myers, &old_tokens, &new_tokens);
    let start = |tokens: &[&str], ix: usize, text: &str| {
        tokens.get(ix).map_or(text.len(), |token| {
            token.as_ptr() as usize - text.as_ptr() as usize
        })
    };
    let (mut old_runs, mut new_runs) = (Vec::new(), Vec::new());
    for op in ops {
        let (old_range, new_range) = (op.old_range(), op.new_range());
        if !old_range.is_empty() && op.tag() != similar::DiffTag::Equal {
            let range =
                start(&old_tokens, old_range.start, old)..start(&old_tokens, old_range.end, old);
            push_run(&mut old_runs, range, old);
        }
        if !new_range.is_empty() && op.tag() != similar::DiffTag::Equal {
            let range =
                start(&new_tokens, new_range.start, new)..start(&new_tokens, new_range.end, new);
            push_run(&mut new_runs, range, new);
        }
    }
    (old_runs, new_runs)
}

/// A line's words, whitespace and other characters, in order, as slices of it.
fn tokens(text: &str) -> Vec<&str> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut tokens = Vec::new();
    let mut start: Option<(usize, Class)> = None;
    for (ix, c) in text.char_indices() {
        let kind = class(c);
        match start {
            Some((_, current)) if current == kind && kind != Class::Other => {}
            Some((from, _)) => {
                tokens.push(&text[from..ix]);
                start = Some((ix, kind));
            }
            None => start = Some((ix, kind)),
        }
    }
    if let Some((from, _)) = start {
        tokens.push(&text[from..]);
    }
    tokens
}

/// Extends the last run when only whitespace separates it from `range`; whitespace alone isn't
/// a run.
fn push_run(runs: &mut Vec<Range<u32>>, range: Range<usize>, text: &str) {
    if text[range.clone()].trim().is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut() {
        let gap = &text[last.end as usize..range.start];
        if gap.trim().is_empty() {
            last.end = range.end as u32;
            return;
        }
    }
    runs.push(range.start as u32..range.end as u32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitto_ui::diff::diff_metadata::parse_patch_files;

    fn names(highlights: &DiffHighlights, diff: &FileDiff, side: Side, ix: usize) -> Vec<String> {
        let line = diff.lines(side).line(ix);
        highlights
            .syntax(side, ix)
            .iter()
            .map(|(range, name)| {
                format!(
                    "{}={}",
                    &line[range.start as usize..range.end as usize],
                    highlight_name(*name)
                )
            })
            .collect()
    }

    #[test]
    fn highlights_each_hunk_on_its_own() {
        // Hunks are separated by lines the patch doesn't have: parsed together, `/* open` and
        // `close */` would be one comment.
        let diff = parse_patch_files(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-let a = 1;\n+let a = 2;\n /* open\n@@ -10,2 +10,2 @@\n close */\n-fn old() {}\n+fn new() {}\n",
            Some("test"),
        )
        .remove(0);
        let highlights = DiffHighlights::compute(&diff);
        assert!(names(&highlights, &diff, Side::Additions, 0).contains(&"let=keyword".to_owned()));
        assert!(
            !names(&highlights, &diff, Side::Additions, 2)
                .iter()
                .any(|name| name.ends_with("=comment"))
        );
        assert!(
            names(&highlights, &diff, Side::Additions, 3)
                .iter()
                .any(|name| name.starts_with("new="))
        );
    }

    #[test]
    fn leaves_a_file_of_unknown_type_plain() {
        let diff = parse_patch_files(
            "diff --git a/notes b/notes\n--- a/notes\n+++ b/notes\n@@ -1 +1 @@\n-a\n+b\n",
            Some("test"),
        )
        .remove(0);
        assert!(
            DiffHighlights::compute(&diff)
                .syntax(Side::Additions, 0)
                .is_empty()
        );
    }

    #[test]
    fn marks_the_words_that_changed_in_a_line_paired_with_another() {
        let (old, new) = changed_runs("let total = price * count;", "let total = cost * quantity;");
        assert_eq!(old, [12..17, 20..25]);
        assert_eq!(new, [12..16, 19..27]);
        // Nothing in common: all of both.
        let ends = |runs: Vec<Range<u32>>| -> Vec<(u32, u32)> {
            runs.into_iter().map(|run| (run.start, run.end)).collect()
        };
        let (old, new) = changed_runs("line2", "main change");
        assert_eq!((ends(old), ends(new)), (vec![(0, 5)], vec![(0, 11)]));
        // Punctuation is a token of its own, so only what changed in a call is marked.
        let (old, new) = changed_runs("value = compute(1);", "value = compute(22);");
        assert_eq!((ends(old), ends(new)), (vec![(16, 17)], vec![(16, 18)]));
    }
}
