/**
 * `name` as the name of a new repository's folder, trimmed; empty while it isn't one: a folder's
 * name has no slashes, and isn't `.` or `..`, which are the folder it'd go in or the one above.
 */
export function folderName(name: string): string {
  const trimmed = name.trim();
  return /[\\/]/.test(trimmed) || /^\.+$/.test(trimmed) ? "" : trimmed;
}
