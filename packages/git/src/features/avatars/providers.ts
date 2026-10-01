import { createHash } from "node:crypto";

import type { Hosting } from "./remote";

/** Requested at twice the largest size it's shown at, so it stays sharp on high-DPI screens. */
export const AVATAR_SIZE = 80;

/** `12345+name@users.noreply.github.com`, or the older `name@users.noreply.github.com`. */
const GITHUB_NOREPLY = /^(?:(\d+)\+)?([^@]+)@users\.noreply\.github\.com$/;

const TIMEOUT = 10_000;

export interface Author {
  /** Normalised: trimmed and lowercase. */
  email: string;
  /** One of their commits, for hosts that only link commits (not addresses) to accounts. */
  sha: string;
}

type Fetch = typeof globalThis.fetch;

/**
 * Looks up authors' avatars on the host of their repository: their account's avatar on GitHub or
 * GitLab, falling back to their Gravatar. The lookups are unauthenticated, so they only see public
 * repositories and public profiles. Results are kept for as long as the app runs.
 */
export class AvatarLookup {
  /** Avatars found (or found missing), by host and email address. */
  private readonly known = new Map<string, string | undefined>();
  private readonly pending = new Map<string, Promise<string | undefined>>();
  /** The authors of a GitHub repository's latest commits, by `owner/name`. */
  private readonly githubRecent = new Map<string, Promise<Map<string, string>>>();
  /** Until when GitHub's rate limit for unauthenticated requests is used up. */
  private githubBlockedUntil = 0;

  constructor(private readonly fetch: Fetch = globalThis.fetch) {}

  /** The author's avatar URL, or a Gravatar URL that 404s if they don't have one either. */
  async find(hosting: Hosting | undefined, author: Author): Promise<string | null> {
    const email = author.email;
    if (!email.includes("@")) return null;
    const noreply = GITHUB_NOREPLY.exec(email);
    if (noreply) return githubNoreplyAvatar(noreply[1], noreply[2]!);

    const found = hosting && (await this.fromHost(hosting, author));
    return found ?? gravatar(email);
  }

  private fromHost(hosting: Hosting, author: Author): Promise<string | undefined> {
    const key = `${hostingKey(hosting)} ${author.email}`;
    if (this.known.has(key)) return Promise.resolve(this.known.get(key));
    let lookup = this.pending.get(key);
    if (!lookup) {
      lookup = (
        hosting.provider === "github"
          ? this.fromGitHub(hosting.owner, hosting.name, author)
          : this.fromGitLab(hosting.host, author.email)
      )
        .then(
          (url) => {
            // Inconclusive (offline, rate limited) lookups are tried again next time.
            if (url !== INCONCLUSIVE) this.known.set(key, url);
            return url === INCONCLUSIVE ? undefined : url;
          },
          () => undefined,
        )
        .finally(() => this.pending.delete(key));
      this.pending.set(key, lookup);
    }
    return lookup;
  }

  /**
   * GitHub has no lookup by email address, only commits linked to the account that wrote them. The
   * repository's latest commits cover most authors in one request; anyone else's avatar is found
   * through one of their own commits.
   */
  private async fromGitHub(owner: string, name: string, author: Author): Promise<Found> {
    const repository = `${encodeURIComponent(owner)}/${encodeURIComponent(name)}`;
    const recent = await this.githubRecentAuthors(repository);
    const url = recent?.get(author.email);
    if (url) return url;

    const commit = await this.github<GitHubCommit>(`repos/${repository}/commits/${author.sha}`);
    if (commit === INCONCLUSIVE) return INCONCLUSIVE;
    return commit?.author?.avatar_url ?? undefined;
  }

  private githubRecentAuthors(repository: string): Promise<Map<string, string> | undefined> {
    let recent = this.githubRecent.get(repository);
    if (!recent) {
      recent = this.github<GitHubCommit[]>(`repos/${repository}/commits?per_page=100`).then(
        (commits) => {
          if (commits === INCONCLUSIVE) throw new Error("GitHub didn't answer.");
          const authors = new Map<string, string>();
          for (const commit of commits ?? []) {
            const email = commit.commit?.author?.email?.trim().toLowerCase();
            const url = commit.author?.avatar_url;
            if (email && url && !authors.has(email)) authors.set(email, url);
          }
          return authors;
        },
      );
      // Asked again next time if GitHub didn't answer.
      recent.catch(() => this.githubRecent.delete(repository));
      this.githubRecent.set(repository, recent);
    }
    return recent.catch(() => undefined);
  }

  /** A GitHub API response; `undefined` when it isn't there (or is private). */
  private async github<T>(path: string): Promise<T | undefined | typeof INCONCLUSIVE> {
    if (Date.now() < this.githubBlockedUntil) return INCONCLUSIVE;
    const response = await this.request(`https://api.github.com/${path}`, {
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
    });
    if (!response) return INCONCLUSIVE;
    if (response.status === 403 || response.status === 429) {
      const reset = Number(response.headers.get("x-ratelimit-reset"));
      this.githubBlockedUntil =
        response.headers.get("x-ratelimit-remaining") === "0" && reset
          ? reset * 1000
          : Date.now() + 60_000;
      return INCONCLUSIVE;
    }
    if (response.status >= 500) return INCONCLUSIVE;
    if (!response.ok) return undefined;
    return (await response.json()) as T;
  }

  /** GitLab looks avatars up by email address, for users whose address is public. */
  private async fromGitLab(host: string, email: string): Promise<Found> {
    const url = `https://${host}/api/v4/avatar?email=${encodeURIComponent(email)}&size=${AVATAR_SIZE}`;
    const response = await this.request(url, { Accept: "application/json" });
    if (!response || response.status === 429 || response.status >= 500) return INCONCLUSIVE;
    if (!response.ok) return undefined;
    const { avatar_url: avatar } = (await response.json()) as { avatar_url?: string | null };
    if (!avatar) return undefined;
    const resolved = new URL(avatar, `https://${host}`);
    // No account with that address: GitLab answers with a Gravatar that has a generated default
    // image, or its own placeholder. Both are left to `gravatar`, which shows initials instead.
    if (resolved.hostname.endsWith("gravatar.com") || resolved.pathname.includes("no_avatar")) {
      return undefined;
    }
    return resolved.href;
  }

  private request(url: string, headers: Record<string, string>): Promise<Response | undefined> {
    return this.fetch(url, {
      headers: { "User-Agent": "Gitto", ...headers },
      signal: AbortSignal.timeout(TIMEOUT),
    }).catch(() => undefined);
  }
}

/** A lookup that couldn't tell, e.g. offline or rate limited. */
const INCONCLUSIVE = Symbol("inconclusive");
type Found = string | undefined | typeof INCONCLUSIVE;

interface GitHubCommit {
  /** The GitHub account the commit's author address belongs to, if any. */
  author?: { avatar_url?: string } | null;
  commit?: { author?: { email?: string } | null };
}

function hostingKey(hosting: Hosting): string {
  return hosting.provider === "github"
    ? `github.com/${hosting.owner}/${hosting.name}`.toLowerCase()
    : `${hosting.host}/${hosting.project}`.toLowerCase();
}

function githubNoreplyAvatar(id: string | undefined, login: string): string {
  return id
    ? `https://avatars.githubusercontent.com/u/${id}?s=${AVATAR_SIZE}&v=4`
    : `https://github.com/${encodeURIComponent(login)}.png?size=${AVATAR_SIZE}`;
}

/** Asks for a 404 rather than a default image, so the initials stay when there's none. */
function gravatar(email: string): string {
  const hash = createHash("sha256").update(email).digest("hex");
  return `https://www.gravatar.com/avatar/${hash}?s=${AVATAR_SIZE}&d=404`;
}
