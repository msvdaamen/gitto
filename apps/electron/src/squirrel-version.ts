/**
 * The version of Gitto's Squirrel package on Windows. NuGet's has a pre-release part of at most 20
 * characters, which electron-winstaller takes the dots out of, and Squirrel compares it as text. A
 * nightly's is only compared with other nightlies', so it's when it was built, which is always as
 * long, and later for every newer nightly: 1.2.4-nightly.20261005134259 is 1.2.4-n20261005134259,
 * and 1.3.0-rc.1.nightly.20261005134259 is 1.3.0-n20261005134259. Any other version is kept.
 */
export function squirrelVersion(version: string) {
  const nightly = /^(\d+\.\d+\.\d+)-(?:.+\.)?nightly\.(\d+)$/.exec(version);
  return nightly ? `${nightly[1]}-n${nightly[2]}` : version;
}
