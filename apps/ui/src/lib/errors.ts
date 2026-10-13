/** What went wrong, in words: an error's message, or whatever else was thrown, as text. */
export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Whether `error` is one the main process sent with the code `code` (see `toApiError`). */
export function hasCode(error: unknown, code: string): boolean {
  return (error as { code?: unknown } | null)?.code === code;
}
