import { createEffect, createSignal, onCleanup, type Accessor } from "solid-js";

/** Requested at twice the largest size it's shown at, so it stays sharp on high-DPI screens. */
const SIZE = 80;

/** `12345+name@users.noreply.github.com`, or the older `name@users.noreply.github.com`. */
const GITHUB_NOREPLY = /^(?:(\d+)\+)?([^@]+)@users\.noreply\.github\.com$/;

/** URLs being looked up, and those already known, by normalised email address. */
const pending = new Map<string, Promise<string | undefined>>();
const known = new Map<string, string | undefined>();
/** Pictures that failed to load (no profile picture, or offline), so they aren't tried again. */
const [failed, setFailed] = createSignal(new Set<string>());

/**
 * The profile picture for an email address, if it might have one: its GitHub avatar for a GitHub
 * noreply address, its Gravatar otherwise. Gravatar is asked for a 404 rather than a default
 * image, so the picture fails to load when there's none and the initials stay.
 */
export function avatarUrl(email: string): Promise<string | undefined> {
  const key = normalize(email);
  let url = pending.get(key);
  if (!url) {
    url = resolveAvatarUrl(key).then((value) => {
      known.set(key, value);
      return value;
    });
    pending.set(key, url);
  }
  return url;
}

function normalize(email: string): string {
  return email.trim().toLowerCase();
}

async function resolveAvatarUrl(email: string): Promise<string | undefined> {
  if (!email.includes("@")) return undefined;
  const github = GITHUB_NOREPLY.exec(email);
  if (github) {
    const [, id, login] = github;
    return id
      ? `https://avatars.githubusercontent.com/u/${id}?s=${SIZE}&v=4`
      : `https://github.com/${encodeURIComponent(login!)}.png?size=${SIZE}`;
  }
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(email));
  const hash = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0"));
  return `https://www.gravatar.com/avatar/${hash.join("")}?s=${SIZE}&d=404`;
}

/**
 * The profile picture to show for `email`, or `undefined` while it's being looked up, when there
 * is none, or once it failed to load. Call `onError` when the picture fails to load.
 */
export function useAvatar(email: () => string | undefined): {
  url: Accessor<string | undefined>;
  onError: () => void;
} {
  // Not a resource: reading one would suspend the history table while the URL is looked up.
  const [resolved, setResolved] = createSignal<string>();
  createEffect(() => {
    const address = email();
    // Known URLs are set right away, so a row that's reused for another commit doesn't flash its
    // initials in between.
    setResolved(address ? known.get(normalize(address)) : undefined);
    if (!address || known.has(normalize(address))) return;
    let current = true;
    onCleanup(() => (current = false));
    void avatarUrl(address).then((value) => current && setResolved(value));
  });
  const url = () => {
    const value = resolved();
    return value && !failed().has(value) ? value : undefined;
  };
  return {
    url,
    onError: () => {
      const value = url();
      if (value) setFailed((set) => new Set(set).add(value));
    },
  };
}
