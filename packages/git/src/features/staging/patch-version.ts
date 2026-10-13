// Names a file's patch by what's in it, so the renderer can say which patch it picked lines from
// without sending it back (see `stageLines`), and name it in its caches. Shared with the renderer,
// so this imports nothing.

/**
 * Names `patch` by what's in it: its length, a hash of its text, and its header, which has the
 * object names of both sides on its `index` line (git hashes the working tree's side for it).
 * Another patch of the same file, as after a save, gets another name.
 */
export function patchVersion(patch: string): string {
  return `${patch.length}:${hash(patch)}:${patchHeader(patch)}`;
}

/** `patch` up to its first hunk: the whole of it when it has none. */
function patchHeader(patch: string): string {
  const body = patch.indexOf("\n@@ ");
  return body === -1 ? patch : patch.slice(0, body);
}

/** FNV-1a over `text`'s characters, in base 36: a few milliseconds for a patch of megabytes. */
function hash(text: string): string {
  let value = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    value = Math.imul(value ^ text.charCodeAt(i), 0x01000193);
  }
  return (value >>> 0).toString(36);
}
