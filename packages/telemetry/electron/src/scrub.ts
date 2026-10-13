/**
 * Takes what identifies the user out of a report's text, wherever it is in it: their home folder,
 * which names them and leads to every repository's path, becomes `~`, and the user and password in
 * a URL, like a remote's with a token in it, become `[redacted]`. Changes `event` in place.
 */
export function scrubber(home: string) {
  // Git and Windows write its path with either separator.
  const homes = [...new Set([home, home.replaceAll("\\", "/")])].filter((path) => path.length > 1);
  const homePattern = homes.length
    ? // Not another user's whose name starts with this one's: /Users/mischa in /Users/mischael.
      new RegExp(`(?:${homes.map(escapeRegExp).join("|")})(?![\\w.-])`, "gi")
    : undefined;

  const scrubText = (text: string) => {
    const scrubbed = text.replace(CREDENTIALS, "$1[redacted]@");
    return homePattern ? scrubbed.replace(homePattern, "~") : scrubbed;
  };

  return <T>(event: T): T => scrubValue(event, scrubText, new WeakSet()) as T;
}

/** A URL's scheme, then its user and password: `user:token@`, or just `token@`. */
const CREDENTIALS = /\b([a-z][a-z\d+.-]*:\/\/)[^\s/@]+@/gi;

function scrubValue(value: unknown, scrubText: (text: string) => string, seen: WeakSet<object>) {
  if (typeof value === "string") return scrubText(value);
  if (typeof value !== "object" || value === null || seen.has(value)) return value;
  seen.add(value);
  const record = value as Record<string, unknown>;
  for (const key of Object.keys(record)) record[key] = scrubValue(record[key], scrubText, seen);
  return value;
}

function escapeRegExp(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
