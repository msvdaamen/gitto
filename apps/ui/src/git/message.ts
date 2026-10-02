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
  const blankLine = /\n\s*\n/.exec(message);
  const subject = blankLine ? message.slice(0, blankLine.index) : message;
  const body = blankLine ? message.slice(blankLine.index + blankLine[0].length) : "";
  return { summary: subject.replace(/\s*\n\s*/g, " ").trim(), description: trimBody(body) };
}

/** The message to commit: the summary, then a blank line and the description if there is one. */
export function joinMessage({ summary, description }: CommitMessage): string {
  const body = trimBody(description);
  return body ? `${summary.trim()}\n\n${body}` : summary.trim();
}

/** Drops blank lines around the body, but keeps its first line's indentation, e.g. for code. */
function trimBody(body: string): string {
  return body.replace(/^\s*\n/, "").trimEnd();
}
