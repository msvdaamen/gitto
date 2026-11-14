// Where to suggest cloning a repository to, from its URL.

/**
 * The name of the repository at `url`, as `git clone` names its folder: the last part of its
 * path, without `.git`. E.g. `gitto` for `https://github.com/owner/gitto.git`,
 * `git@github.com:owner/gitto.git` or `/srv/git/gitto/`; empty if there's none yet.
 */
export function repositoryName(url: string): string {
  const path = url
    .trim()
    // Without a query or fragment, and the slashes and `.git` at the end.
    .replace(/[?#].*$/, "")
    .replace(/[\\/]+$/, "")
    .replace(/\.git$/i, "")
    .replace(/[\\/]+$/, "");
  // After the last slash, or the colon of `host:path`, or of `scheme://host` when there's no path.
  const name = path.split(/[\\/:]/).pop() ?? "";
  return /^[.]*$/.test(name) || /^[\w.-]+@/.test(name) ? "" : name;
}

/** The path separator `path` uses: `\` on Windows, `/` elsewhere. */
function separatorOf(path: string): string {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

/** The folder `path` is in; `undefined` for a root, or a path without a folder. */
export function parentFolder(path: string): string | undefined {
  const separator = separatorOf(path);
  const trimmed = path.length > 1 && path.endsWith(separator) ? path.slice(0, -1) : path;
  const at = trimmed.lastIndexOf(separator);
  if (at < 0) return undefined;
  // `/home` is in `/`, and `C:\code` in `C:\`.
  const parent = trimmed.slice(0, at);
  return parent === "" || /^[A-Za-z]:$/.test(parent) ? parent + separator : parent;
}

/** `name` in the folder `parent`, with the separator `parent` uses. */
export function inFolder(parent: string, name: string): string {
  const separator = separatorOf(parent);
  return parent.endsWith(separator) ? parent + name : `${parent}${separator}${name}`;
}
