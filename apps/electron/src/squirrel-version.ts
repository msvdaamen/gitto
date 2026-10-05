/**
 * The version of Gitto's Squirrel package on Windows. NuGet's has a pre-release part of at most 20
 * characters, which electron-winstaller takes the dots out of, and Squirrel compares the number it
 * ends with as an Int32 when the letters before it are the same. A nightly's is only compared with
 * other nightlies', so it's the minutes from 1970 to when it was built, which only grow, and have 8
 * digits until 2160 (so text sorts them the same): 1.2.4-nightly.20261005134259 is
 * 1.2.4-nightly29853462, and 1.3.0-rc.1.nightly.20261005134259 is 1.3.0-nightly29853462. Any other
 * version is kept.
 */
export function squirrelVersion(version: string) {
  const nightly =
    /^(\d+\.\d+\.\d+)-(?:.+\.)?nightly\.(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})\d{2}$/.exec(version);
  if (!nightly) return version;
  const [, , year, month, day, hour, minute] = nightly.map(Number);
  const minutes = Date.UTC(year!, month! - 1, day, hour, minute) / 60_000;
  return `${nightly[1]}-nightly${minutes}`;
}
