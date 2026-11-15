import { homedir } from "node:os";
import { isAbsolute, resolve } from "node:path";

/**
 * `folder` as typed or picked, as a full path: with `~` for the home folder spelled out, and
 * without a slash at the end. `undefined` for a path that isn't a full one, like `code/gitto` or
 * `~someone/code`, which would depend on where Gitto happens to run.
 */
export function fullPath(folder: string): string | undefined {
  const trimmed = folder.trim();
  const expanded = /^~(?=$|[\\/])/.test(trimmed) ? homedir() + trimmed.slice(1) : trimmed;
  return isAbsolute(expanded) ? resolve(expanded) : undefined;
}
