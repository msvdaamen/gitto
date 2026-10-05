import type { Activity, ActivityKind, Overview } from "./schema";

// With -z, entries are NUL-separated as well, so the output is a flat list of fields where every
// three make up one entry: the reflog selector (with `--date=unix`, `HEAD@{<seconds>}`), git's
// message for it, and the subject of the commit HEAD moved to.
export const REFLOG_FORMAT = "--format=%gd%x00%gs%x00%s";

/** HEAD's reflog, as `log -g -z --date=unix` prints it with `REFLOG_FORMAT`, as activity. */
export function parseReflog(output: string): Activity[] {
  const fields = output.split("\0");
  const activity: Activity[] = [];
  for (let i = 0; i + 3 <= fields.length; i += 3) {
    const [selector, message, subject] = fields.slice(i, i + 3) as [string, string, string];
    const seconds = /@\{(\d+)\}$/.exec(selector.trim())?.[1];
    if (seconds === undefined || !message) continue;
    const entry = toActivity(message);
    if (entry) activity.push({ at: Number(seconds) * 1000, ...entry, subject, message });
  }
  return activity;
}

/**
 * What a reflog message (e.g. `checkout: moving from main to feature`) says was done; `undefined`
 * for a step of something that's shown once it's done (a rebase's picks), or for what didn't move
 * HEAD (a stash resets it to itself).
 */
function toActivity(message: string): { kind: ActivityKind; target: string | null } | undefined {
  // `<command>: <details>`, the command with its phase if it has steps, e.g. `rebase (pick)`.
  const colon = message.indexOf(": ");
  const command = colon === -1 ? message : message.slice(0, colon);
  const details = colon === -1 ? "" : message.slice(colon + 2);
  const phase = /\(([^)]*)\)$/.exec(command)?.[1];
  const [name, ...args] = command.replace(/\s*\([^)]*\)$/, "").split(" ");
  // What the command was given, but for options: e.g. `origin main` from `pull --ff-only origin main`.
  const operands = args.filter((arg) => !arg.startsWith("-")).join(" ") || null;

  switch (name) {
    case "commit":
      return { kind: phase === "amend" ? "amend" : "commit", target: null };
    case "checkout":
      // Branch names can't have spaces; a detached HEAD is named by its commit.
      return { kind: "checkout", target: / to (\S+)$/.exec(details)?.[1] ?? null };
    case "merge":
      return { kind: "merge", target: operands };
    case "pull":
      // `pull --rebase` goes through the same steps as a rebase.
      if (phase !== undefined && phase !== "finish") return undefined;
      return { kind: "pull", target: operands };
    case "rebase": {
      if (phase !== "finish") return undefined;
      const branch = /^returning to refs\/heads\/(.+)$/.exec(details)?.[1] ?? null;
      return { kind: "rebase", target: branch };
    }
    case "reset": {
      const target = /^moving to (.+)$/.exec(details)?.[1] ?? null;
      // `git stash` resets to HEAD, which leaves it where it was.
      if (target === "HEAD") return undefined;
      return { kind: "reset", target };
    }
    case "cherry-pick":
      return { kind: "cherry-pick", target: null };
    case "revert":
      return { kind: "revert", target: null };
    case "clone":
      return { kind: "clone", target: null };
    default:
      return { kind: "other", target: null };
  }
}

/**
 * The remote a repository is shown as hosted on, from its `remote.<name>.url` settings as
 * `config -z --get-regexp` prints them: `origin`, or else the first one; `null` without any.
 */
export function pickRemote(output: string): Overview["remote"] {
  const remotes = new Map<string, string>();
  for (const entry of output.split("\0")) {
    const newline = entry.indexOf("\n");
    if (newline === -1) continue;
    const name = /^remote\.(.+)\.url$/.exec(entry.slice(0, newline))?.[1];
    // A remote with several URLs fetches from the first.
    if (name !== undefined && !remotes.has(name)) remotes.set(name, entry.slice(newline + 1));
  }
  const name = remotes.has("origin") ? "origin" : remotes.keys().next().value;
  return name === undefined ? null : { name, url: remotes.get(name)! };
}
