import type { Repo } from "../../core/repo";

/** Where a repository is hosted, if it's on a host whose avatars Gitto can look up. */
export type Hosting =
  | { provider: "github"; owner: string; name: string }
  | { provider: "gitlab"; host: string; project: string };

/**
 * The hosting of the repository's `origin` remote, or of its first remote on a known host:
 * github.com, or a GitLab (gitlab.com, or a self-hosted one with "gitlab" in its host name).
 */
export async function getHosting(repo: Repo, signal?: AbortSignal): Promise<Hosting | undefined> {
  // Exits with 1 when there are no remotes.
  const output = await repo
    .read(["config", "--get-regexp", String.raw`^remote\..+\.url$`], { signal })
    .catch(() => "");
  const remotes = output
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [key = "", url = ""] = line.split(" ", 2);
      return { name: key.slice("remote.".length, -".url".length), url };
    });
  const ordered = [
    ...remotes.filter((remote) => remote.name === "origin"),
    ...remotes.filter((remote) => remote.name !== "origin"),
  ];
  for (const remote of ordered) {
    const hosting = parseHosting(remote.url);
    if (hosting) return hosting;
  }
  return undefined;
}

/**
 * `git@github.com:owner/name.git`, `https://github.com/owner/name`,
 * `ssh://git@gitlab.example.com:2222/group/sub/project.git`, … → where it's hosted.
 */
export function parseHosting(url: string): Hosting | undefined {
  const remote = parseRemoteUrl(url);
  if (!remote) return undefined;
  const path = remote.path.replace(/^\/+|\/+$/g, "").replace(/\.git$/, "");
  const host = remote.host.toLowerCase();

  if (host === "github.com" || host === "www.github.com") {
    const [owner, name, ...rest] = path.split("/");
    if (!owner || !name || rest.length) return undefined;
    return { provider: "github", owner, name };
  }
  if (host.includes("gitlab") && path.includes("/")) {
    return { provider: "gitlab", host, project: path };
  }
  return undefined;
}

function parseRemoteUrl(url: string): { host: string; path: string } | undefined {
  // scp-like syntax: `[user@]host:path`, with no scheme.
  const scp = /^(?:[^@/]+@)?([^:/]+):(?!\/\/)(.+)$/.exec(url);
  if (scp && !url.includes("://")) return { host: scp[1]!, path: scp[2]! };
  try {
    const parsed = new URL(url);
    if (!["https:", "http:", "ssh:", "git:"].includes(parsed.protocol)) return undefined;
    return { host: parsed.hostname, path: decodeURIComponent(parsed.pathname) };
  } catch {
    return undefined;
  }
}
