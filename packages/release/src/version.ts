/** As the release workflow takes them: a pre-release part, if any, starts with a letter. */
export const SEMVER =
  /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z][0-9A-Za-z-]*(\.[0-9A-Za-z-]+)*)?$/;

/** A nightly updates to the next nightly, a release to the next release. */
export type Channel = "nightly" | "release";

/** A nightly's version is `nightly` and when it was built, in minutes: 1.2.4-nightly29853462. */
export function channelOf(version: string): Channel {
  return prerelease(version).some((part) => /^nightly\d+$/.test(part)) ? "nightly" : "release";
}

/** Whether it's a pre-release, like a nightly or 1.3.0-beta.1. */
export function isPrerelease(version: string) {
  return prerelease(version).length > 0;
}

/** Compares two versions by semver's precedence: negative if `a` comes before `b`, positive if after. */
export function compareVersions(a: string, b: string): number {
  const [aCore, bCore] = [core(a), core(b)];
  for (let i = 0; i < 3; i++) {
    if (aCore[i] !== bCore[i]) return aCore[i]! - bCore[i]!;
  }
  const [aPre, bPre] = [prerelease(a), prerelease(b)];
  // A pre-release comes before its release.
  if (!aPre.length || !bPre.length) return bPre.length - aPre.length;
  for (let i = 0; i < Math.min(aPre.length, bPre.length); i++) {
    const order = compareIdentifiers(aPre[i]!, bPre[i]!);
    if (order) return order;
  }
  return aPre.length - bPre.length;
}

function core(version: string) {
  return version.split(/[-+]/, 1)[0]!.split(".").map(Number);
}

function prerelease(version: string) {
  const plain = version.split("+", 1)[0]!;
  const dash = plain.indexOf("-");
  return dash === -1 ? [] : plain.slice(dash + 1).split(".");
}

function compareIdentifiers(a: string, b: string) {
  const [aNumber, bNumber] = [/^\d+$/.test(a), /^\d+$/.test(b)];
  if (aNumber && bNumber) return Number(a) - Number(b);
  // Numbers come before words.
  if (aNumber || bNumber) return aNumber ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}
