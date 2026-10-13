// Shared with the renderer, which names a remote branch's remote the same way, so this imports
// nothing.

/**
 * The remote, among `remotes`, the remote branch `name` (e.g. `origin/feature`) is on: the longest
 * one it starts with, as a remote's name can have a slash in it. `undefined` when it's gone, its
 * branches left.
 */
export function remoteOf(remotes: readonly string[], name: string): string | undefined {
  return remotes
    .filter((remote) => remote && name.startsWith(`${remote}/`))
    .reduce<string | undefined>(
      (longest, remote) => (remote.length > (longest?.length ?? 0) ? remote : longest),
      undefined,
    );
}
