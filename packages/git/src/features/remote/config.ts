import { GitError } from "../../core/errors";
import type { GitCommand } from "../../core/repo";

/**
 * The settings a pull or push depends on, by key. Git lowercases the section and name, but not a
 * branch's name or a URL: e.g. `branch.Feature.merge`.
 */
export class RemoteConfig {
  private readonly values = new Map<string, string[]>();

  /** The value `key` is set to; the last, if it's set several times, as git takes it. */
  get(key: string): string | undefined {
    return this.values.get(key)?.at(-1);
  }

  /** Every value `key` is set to, in order: a branch's `merge` can name several branches. */
  all(key: string): string[] {
    return this.values.get(key) ?? [];
  }

  has(key: string): boolean {
    return this.values.has(key);
  }

  keys(): IterableIterator<string> {
    return this.values.keys();
  }

  add(key: string, value: string): void {
    const values = this.values.get(key);
    if (values) values.push(value);
    else this.values.set(key, [value]);
  }
}

/** The settings a pull or push depends on, in one read. */
export async function readConfig(run: GitCommand): Promise<RemoteConfig> {
  const pattern = String.raw`^(pull\.(rebase|ff)|push\.default|remote\.pushdefault|branch\..+\.(remote|merge|rebase|pushremote)|remote\..+\.(url|pushurl|fetch|push|skipfetchall))$`;
  let output = "";
  try {
    output = await run(["config", "-z", "--get-regexp", pattern]);
  } catch (error) {
    // None of them is set.
    if (!(error instanceof GitError && error.exitCode === 1)) throw error;
  }
  return parseConfig(output);
}

/** The settings in `git config -z --get-regexp`'s output: each entry a key, a newline and its value. */
export function parseConfig(output: string): RemoteConfig {
  const config = new RemoteConfig();
  for (const entry of output.split("\0")) {
    if (!entry) continue;
    const newline = entry.indexOf("\n");
    // A boolean set without a value (`[pull] rebase`) has no newline: it's true.
    if (newline === -1) config.add(entry, "true");
    else config.add(entry.slice(0, newline), entry.slice(newline + 1));
  }
  return config;
}

/** The values git takes for false, lowercased; an empty one is false too. */
export const FALSE = new Set(["false", "no", "off", "0", ""]);

/**
 * The remotes `git fetch --all` fetches (those not set to be skipped), going by `config`, but for
 * `except`, which a pull already fetched.
 */
export function remotesToFetch(config: RemoteConfig, except: string): string[] {
  return remoteNames(config, ["url"]).filter((name) => {
    const skip = config.get(`remote.${name}.skipfetchall`)?.toLowerCase();
    return name !== except && (skip === undefined || FALSE.has(skip));
  });
}

/**
 * The remotes that have one of `settings` (e.g. `url`, which fetching needs), by name, as
 * `config` has them.
 */
export function remoteNames(config: RemoteConfig, settings: string[]): string[] {
  const names = new Set<string>();
  for (const key of config.keys()) {
    const [, name, setting] = /^remote\.(.+)\.([^.]+)$/.exec(key) ?? [];
    if (name !== undefined && settings.includes(setting!)) names.add(name);
  }
  return [...names];
}

/** How `branch` is set to be rebased when pulled, lowercased; `undefined` when it isn't set. */
export function rebaseSetting(config: RemoteConfig, branch: string): string | undefined {
  return (config.get(`branch.${branch}.rebase`) ?? config.get("pull.rebase"))?.toLowerCase();
}

/** Whether a pull of `branch` can make a merge commit, going by `config`. */
export function makesMergeCommit(config: RemoteConfig, branch: string): boolean {
  return !rebases(config, branch) && config.get("pull.ff")?.toLowerCase() !== "only";
}

/** Whether a pull of `branch` rebases, going by `config`: as in `git pull`, `pull.ff=only` wins. */
export function rebases(config: RemoteConfig, branch: string): boolean {
  const rebase = rebaseSetting(config, branch);
  return (
    config.get("pull.ff")?.toLowerCase() !== "only" && rebase !== undefined && !FALSE.has(rebase)
  );
}

/**
 * The remote `branch` is pushed to, as `git push` picks it: the branch's `pushRemote`, the
 * repository's `pushDefault`, or the remote it pulls from (unless that's the repository itself,
 * "."), by name or as a URL. Without any, the only remote, or `origin` among several; `undefined`
 * if there's none.
 */
export function pushRemote(config: RemoteConfig, branch: string): string | undefined {
  const upstream = config.get(`branch.${branch}.remote`);
  const set =
    config.get(`branch.${branch}.pushremote`) ??
    config.get("remote.pushdefault") ??
    (upstream === "." ? undefined : upstream);
  if (set) return set;
  const remotes = remoteNames(config, ["url", "pushurl"]);
  if (remotes.length === 1) return remotes[0];
  return remotes.includes("origin") ? "origin" : undefined;
}

/**
 * Where `ref` (a full ref name, e.g. `refs/heads/main`) goes through `refspecs`, a remote's `fetch`
 * or `push` settings, as git maps it: e.g. to `refs/remotes/origin/main` through
 * `+refs/heads/*:refs/remotes/origin/*`. `undefined` if none of them takes it, or a negative one
 * (`^…`) leaves it out. A refspec without a destination takes a ref to the same name when
 * `pushing`, and nowhere when fetching. When `pushing`, `ref` is the branch checked out, which a
 * refspec can name as `HEAD`, and names that aren't full are taken for branches', as git does.
 */
export function mapRef(refspecs: string[], ref: string, pushing = false): string | undefined {
  const full = (name: string) =>
    !pushing || name.startsWith("refs/") ? name : name === "HEAD" ? ref : `refs/heads/${name}`;
  const specs = refspecs.map((spec) => spec.replace(/^\+/, ""));
  if (specs.some((spec) => spec.startsWith("^") && match(full(spec.slice(1)), ref) !== undefined)) {
    return undefined;
  }
  for (const spec of specs) {
    if (spec.startsWith("^")) continue;
    // `:` alone pushes each branch to the same name.
    if (spec === ":") {
      if (pushing) return ref;
      continue;
    }
    const [src = "", dst = pushing ? src : ""] = spec.split(":");
    const star = match(full(src), ref);
    if (star !== undefined && dst) return full(dst).replace("*", star);
  }
  return undefined;
}

/** What `pattern`'s `*` stands for in `ref` if it matches (`""` if it has none), or `undefined`. */
function match(pattern: string, ref: string): string | undefined {
  const star = pattern.indexOf("*");
  if (star === -1) return pattern === ref ? "" : undefined;
  const [before, after] = [pattern.slice(0, star), pattern.slice(star + 1)];
  if (ref.length < before.length + after.length) return undefined;
  if (!ref.startsWith(before) || !ref.endsWith(after)) return undefined;
  return ref.slice(before.length, ref.length - after.length);
}
