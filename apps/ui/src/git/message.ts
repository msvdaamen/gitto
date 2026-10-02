/** A commit message as the commit form edits it: a one-line summary, and the rest. */
export interface CommitMessage {
  summary: string;
  description: string;
}

export const emptyMessage: CommitMessage = { summary: "", description: "" };

// Whitespace as git sees it: ASCII only, so e.g. a line of a no-break space isn't blank.
const LEADING_BLANK_LINES = /^(?:[ \t\v\f\r]*\n)+/;
const BLANK_LINES = /\n(?:[ \t\v\f\r]*\n)+/;
const LINE_BREAK = /[ \t\v\f\r]*\n[ \t\v\f\r]*/g;
const trim = (text: string) => text.replace(/^[ \t\n\v\f\r]+|[ \t\n\v\f\r]+$/g, "");

/**
 * Splits a commit message into its subject, the first paragraph on one line as git shows it, and
 * the body after it.
 */
export function splitMessage(message: string): CommitMessage {
  const { subject, body } = parts(message);
  return { summary: trim(subject.replace(LINE_BREAK, " ")), description: trimBody(body) };
}

/** The message to commit: the summary, then a blank line and the description if there is one. */
export function joinMessage({ summary, description }: CommitMessage): string {
  const body = trimBody(description);
  return body ? `${trim(summary)}\n\n${body}` : trim(summary);
}

/**
 * `original` with the edits made to it as `splitMessage` split it. A part left as it was, or only
 * changed around its ends, is kept exactly as written, e.g. a subject over several lines when only
 * the description changed.
 */
export function editMessage(original: string, edited: CommitMessage): string {
  const { subject, separator, body } = parts(original);
  const split = splitMessage(original);
  const newSummary = trim(edited.summary);
  const newDescription = trimBody(edited.description);
  if (newSummary === split.summary && newDescription === split.description) return original;
  const summary = newSummary === split.summary ? subject : newSummary;
  const description = newDescription === split.description ? body : newDescription;
  return trim(description) ? `${summary}${separator}${description}` : summary;
}

/**
 * The first paragraph, the blank lines after it, and the rest, as written. Blank lines before the
 * first paragraph are skipped, as git does.
 */
function parts(written: string) {
  const message = written.replace(LEADING_BLANK_LINES, "");
  const blankLines = BLANK_LINES.exec(message);
  if (!blankLines) return { subject: message.replace(/\n+$/, ""), separator: "\n\n", body: "" };
  const end = blankLines.index + blankLines[0].length;
  return {
    subject: message.slice(0, blankLines.index),
    separator: blankLines[0],
    body: message.slice(end),
  };
}

/** Drops blank lines around the body, but keeps its first line's indentation, e.g. for code. */
function trimBody(body: string): string {
  return body.replace(LEADING_BLANK_LINES, "").replace(/[ \t\n\v\f\r]+$/, "");
}
