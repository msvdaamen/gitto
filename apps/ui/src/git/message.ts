/** A commit message as the commit form edits it: a one-line summary, and the rest. */
export interface CommitMessage {
  summary: string;
  description: string;
}

export const emptyMessage: CommitMessage = { summary: "", description: "" };

/** Splits a commit message into its first line and the rest. */
export function splitMessage(message: string): CommitMessage {
  const [summary = "", ...rest] = message.split("\n");
  return { summary, description: rest.join("\n").trim() };
}

/** The message to commit: the summary, then a blank line and the description if there is one. */
export function joinMessage({ summary, description }: CommitMessage): string {
  const body = description.trim();
  return body ? `${summary.trim()}\n\n${body}` : summary.trim();
}
