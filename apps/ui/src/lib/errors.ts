/** What went wrong, in words: an error's message, or whatever else was thrown, as text. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
