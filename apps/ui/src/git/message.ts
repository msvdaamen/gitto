/** A commit message as the commit form edits it: a one-line summary, and the rest. */
export interface CommitMessage {
  summary: string;
  description: string;
}

export const emptyMessage: CommitMessage = { summary: "", description: "" };

/**
 * Splits a commit message into its subject, the first paragraph on one line as git shows it, and
 * the body after it.
 */
export function splitMessage(message: string): CommitMessage {
  const { subject, body } = parts(message);
  return { summary: subject.replace(/\s*\n\s*/g, " ").trim(), description: trimBody(body) };
}

/** The message to commit: the summary, then a blank line and the description if there is one. */
export function joinMessage({ summary, description }: CommitMessage): string {
  const body = trimBody(description);
  return body ? `${summary.trim()}\n\n${body}` : summary.trim();
}

/**
 * `original` with the edits made to it as `splitMessage` split it. A part left as it was is kept
 * exactly as written, e.g. a subject over several lines when only the description changed.
 */
export function editMessage(original: string, edited: CommitMessage): string {
  const { subject, separator, body } = parts(original);
  const split = splitMessage(original);
  const summary = edited.summary === split.summary ? subject : edited.summary.trim();
  const description =
    edited.description === split.description ? body : trimBody(edited.description);
  return description.trim() ? `${summary}${separator}${description}` : summary;
}

/** The first paragraph, the blank lines after it, and the rest, as written. */
function parts(message: string) {
  const blankLines = /\n\s*\n/.exec(message);
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
  return body.replace(/^\s*\n/, "").trimEnd();
}
