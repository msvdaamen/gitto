/** Hosts known by the name of their service. */
const PROVIDERS: Record<string, string> = {
  "github.com": "GitHub",
  "gitlab.com": "GitLab",
  "bitbucket.org": "Bitbucket",
  "codeberg.org": "Codeberg",
  "dev.azure.com": "Azure DevOps",
};

/**
 * The host of a remote's URL, e.g. `github.com` for `git@github.com:owner/repo.git` or
 * `https://github.com/owner/repo`; `null` for a remote on this machine.
 */
export function remoteHost(url: string): string | null {
  if (/^[a-z][a-z\d+.-]*:\/\//i.test(url)) {
    if (/^file:/i.test(url)) return null;
    try {
      return new URL(url).hostname.toLowerCase() || null;
    } catch {
      return null;
    }
  }
  // scp-like, `[user@]host:path`; a Windows path's drive letter (`C:\`) isn't a host.
  const host = /^(?:[^@/\\]+@)?([^/\\:]{2,}):/.exec(url)?.[1];
  return host?.toLowerCase() ?? null;
}

/** What to call where a remote is hosted: "GitHub", or its host if it isn't a known one. */
export function remoteProvider(url: string): string {
  const host = remoteHost(url);
  if (host === null) return "Local";
  return PROVIDERS[host.replace(/^(www|ssh)\./, "")] ?? host;
}
