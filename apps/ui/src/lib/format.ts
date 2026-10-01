/** Up to two letters for an avatar: `AL` for "Ada Lovelace", `AD` for "ada". */
export function initials(name: string): string {
  const words = name.split(/[\s._-]+/).filter(Boolean);
  const letters = words.length > 1 ? words[0]![0]! + words.at(-1)![0]! : name.slice(0, 2);
  return letters.toUpperCase();
}

const AVATAR_COLORS = ["#7c5ce7", "#38bda9", "#a978dd", "#e2a646", "#5b9be6", "#e0707a"];

/** An avatar colour picked by `key` (e.g. an email address), the same one every time. */
export function avatarColor(key: string): string {
  let hash = 0;
  for (const char of key.toLowerCase()) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length]!;
}

const relativeTimeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

const TIME_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 60 * 60 * 1000],
  ["month", 30 * 24 * 60 * 60 * 1000],
  ["week", 7 * 24 * 60 * 60 * 1000],
  ["day", 24 * 60 * 60 * 1000],
  ["hour", 60 * 60 * 1000],
  ["minute", 60 * 1000],
];

/** `timestamp` (milliseconds since the epoch) relative to `now`, e.g. "3 days ago". */
export function relativeTime(timestamp: number, now = Date.now()): string {
  const diff = timestamp - now;
  for (const [unit, size] of TIME_UNITS) {
    if (Math.abs(diff) >= size) return relativeTimeFormat.format(Math.round(diff / size), unit);
  }
  return "Just now";
}

/** A count short enough for a badge: `1.2k` for 1234. */
export function compactCount(count: number): string {
  if (count < 1000) return String(count);
  return `${(count / 1000).toFixed(count < 10_000 ? 1 : 0).replace(/\.0$/, "")}k`;
}
