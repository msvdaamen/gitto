import { channelOf, compareVersions, isPrerelease, SEMVER } from "./version.ts";

/**
 * What a commit's title starts with, as Conventional Commits names them: `feat: add worktrees`,
 * `fix(diff): …`, or `feat!: …` for a breaking change. Pull requests are squashed, so their title
 * is the commit's.
 */
export const CHANGE_TYPES = [
  "feat",
  "fix",
  "perf",
  "refactor",
  "docs",
  "style",
  "test",
  "build",
  "ci",
  "chore",
  "revert",
] as const;
export type ChangeType = (typeof CHANGE_TYPES)[number];

/** What users see of a version in the app: what's new, fixed or faster. The release notes say all. */
export const USER_FACING: readonly ChangeType[] = ["feat", "fix", "perf"];

export interface Change {
  type: ChangeType;
  scope?: string;
  /** Marked `!` after its type, or with a `BREAKING CHANGE:` footer. */
  breaking: boolean;
  /** The title after its type, without the pull request's number. */
  description: string;
  /** A `Changelog:` line in the commit's message, telling users more than the title does. */
  detail?: string;
  pr?: number;
  commit: string;
}

/** What a version changed since the one before it on its channel. */
export interface VersionChanges {
  version: string;
  /** When it was built, as YYYY-MM-DD. */
  date: string;
  changes: Change[];
}

/** Every version's changes, the newest first. Each build adds its own to the last build's. */
export interface Changelog {
  versions: VersionChanges[];
}

const TITLE = /^([a-z]+)(?:\(([^()\s]+)\))?(!)?: (\S.*)$/;
const PULL_REQUEST = /\s+\(#(\d+)\)$/;

/** A title's type, scope and description, or why it isn't a Conventional Commits title. */
export function parseTitle(
  title: string,
):
  | { ok: true; type: ChangeType; scope?: string; breaking: boolean; description: string }
  | { ok: false; reason: string } {
  const match = TITLE.exec(title.trim());
  if (!match) {
    return {
      ok: false,
      reason: `"${title}" isn't "<type>: <description>", like "feat: add worktrees" or "fix(diff): …".`,
    };
  }
  const [, type, scope, bang, description] = match;
  if (!(CHANGE_TYPES as readonly string[]).includes(type!)) {
    return { ok: false, reason: `"${type}" isn't one of ${CHANGE_TYPES.join(", ")}.` };
  }
  return {
    ok: true,
    type: type as ChangeType,
    ...(scope && { scope }),
    breaking: !!bang,
    description: description!,
  };
}

/** The format `git log` is asked for, which {@link parseLog} reads. */
export const LOG_FORMAT = "%H%x1f%B%x1e";

/** The changes in `git log --format=LOG_FORMAT`'s output; commits with other titles are left out. */
export function parseLog(output: string): Change[] {
  return output
    .split("\x1e")
    .map((record) => record.trim())
    .filter(Boolean)
    .flatMap((record) => {
      const [commit, message = ""] = record.split("\x1f");
      const change = parseCommit(commit!, message);
      return change ? [change] : [];
    });
}

export function parseCommit(commit: string, message: string): Change | null {
  const [title = "", ...lines] = message.trim().split("\n");
  const pr = PULL_REQUEST.exec(title);
  const parsed = parseTitle(pr ? title.slice(0, pr.index) : title);
  if (!parsed.ok) return null;
  const body = lines.join("\n");
  const detail = /^Changelog:[ \t]*(\S.*)$/m.exec(body)?.[1]?.trim();
  return {
    type: parsed.type,
    ...(parsed.scope && { scope: parsed.scope }),
    breaking: parsed.breaking || /^BREAKING[ -]CHANGE:/m.test(body),
    description: parsed.description,
    ...(detail && { detail }),
    ...(pr && { pr: Number(pr[1]) }),
    commit,
  };
}

/** The changelog with a version's changes added, replacing any it had for that version. */
export function addVersion(
  changelog: Changelog,
  version: VersionChanges,
  keep = Infinity,
): Changelog {
  const versions = [version, ...changelog.versions.filter((v) => v.version !== version.version)]
    .toSorted((a, b) => compareVersions(b.version, a.version))
    .slice(0, keep);
  return { versions };
}

/** What a release after these changes bumps: breaking changes the major, features the minor. */
export function bumpFor(changes: Change[]): "major" | "minor" | "patch" {
  if (changes.some((change) => change.breaking)) return "major";
  if (changes.some((change) => change.type === "feat")) return "minor";
  return "patch";
}

/**
 * The tag a version's changes are counted from, the version before it on its channel: for a
 * nightly, the last nightly; for a release, the release before it, leaving out pre-releases
 * (1.3.0's changes are since 1.2.3, not since 1.3.0-rc.1); for another pre-release, the version
 * before it, pre-release or not. Nightlies are left out of releases'. None for the first.
 */
export function previousTag(version: string, tags: string[]): string | undefined {
  const channel = channelOf(version);
  return tags
    .filter((tag) => tag.startsWith("v") && SEMVER.test(tag.slice(1)))
    .map((tag) => tag.slice(1))
    .filter(
      (v) =>
        compareVersions(v, version) < 0 &&
        channelOf(v) === channel &&
        (isPrerelease(version) || !isPrerelease(v)),
    )
    .toSorted(compareVersions)
    .map((v) => `v${v}`)
    .at(-1);
}

/** The nightlies' tags past the newest `keep`, to be deleted with their releases. */
export function staleNightlies(tags: string[], keep: number): string[] {
  return tags
    .filter((tag) => tag.startsWith("v") && SEMVER.test(tag.slice(1)))
    .filter((tag) => channelOf(tag.slice(1)) === "nightly")
    .toSorted((a, b) => compareVersions(b.slice(1), a.slice(1)))
    .slice(keep);
}

const SECTIONS: [string, (change: Change) => boolean][] = [
  ["⚠️ Breaking changes", (change) => change.breaking],
  ["New", (change) => change.type === "feat"],
  ["Fixes", (change) => change.type === "fix"],
  ["Performance", (change) => change.type === "perf"],
  ["Other changes", () => true],
];

/** The changes as Markdown, for the release's notes on GitHub: every type, grouped. */
export function releaseNotes(changes: Change[]): string {
  if (!changes.length) return "Nothing changed since the last build.";
  const left = new Set(changes);
  const sections: string[] = [];
  for (const [heading, belongs] of SECTIONS) {
    const items = [...left].filter(belongs);
    if (!items.length) continue;
    for (const change of items) left.delete(change);
    const other = heading === "Other changes";
    sections.push(`### ${heading}\n\n${items.map((change) => noteFor(change, other)).join("\n")}`);
  }
  return sections.join("\n\n");
}

function noteFor(change: Change, withType: boolean) {
  const scope = change.scope ? `**${change.scope}:** ` : "";
  const description = withType
    ? `${change.type}: ${change.description}`
    : capitalize(change.description);
  const pr = change.pr ? ` (#${change.pr})` : "";
  const detail = change.detail ? `\n  ${change.detail}` : "";
  return `- ${scope}${description}${pr}${detail}`;
}

/** The description as a sentence starts: titles are written `feat: add worktrees`. */
export function capitalize(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * What users see of the versions after `after` up to `upTo`, the newest first: their new, fixed and
 * faster changes, and breaking ones of any type. Versions with none are left out.
 */
export function userFacingChanges(
  changelog: Changelog,
  { after, upTo }: { after?: string; upTo: string },
): VersionChanges[] {
  return changelog.versions
    .filter(
      ({ version }) =>
        compareVersions(version, upTo) <= 0 && (!after || compareVersions(version, after) > 0),
    )
    .toSorted((a, b) => compareVersions(b.version, a.version))
    .map(({ version, date, changes }) => ({
      version,
      date,
      changes: changes.filter((change) => change.breaking || USER_FACING.includes(change.type)),
    }))
    .filter((version) => version.changes.length > 0);
}
