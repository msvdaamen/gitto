import { GitError } from "../../core/errors";
import type { GitCommand } from "../../core/repo";

/**
 * The settings a pull depends on, by key. Git lowercases the section and name, but not a branch's
 * name or a URL: e.g. `branch.Feature.merge`.
 */
export class PullConfig {
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

/** The settings a pull depends on, in one read. */
export async function readConfig(run: GitCommand): Promise<PullConfig> {
  const pattern = String.raw`^(pull\.(rebase|ff)|branch\..+\.(remote|merge|rebase)|remote\..+\.(url|skipfetchall))$`;
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
export function parseConfig(output: string): PullConfig {
  const config = new PullConfig();
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
export function remotesToFetch(config: PullConfig, except: string): string[] {
  const remotes: string[] = [];
  for (const key of config.keys()) {
    const name = /^remote\.(.+)\.url$/.exec(key)?.[1];
    if (name === undefined || name === except) continue;
    const skip = config.get(`remote.${name}.skipfetchall`)?.toLowerCase();
    if (skip === undefined || FALSE.has(skip)) remotes.push(name);
  }
  return remotes;
}

/** How `branch` is set to be rebased when pulled, lowercased; `undefined` when it isn't set. */
export function rebaseSetting(config: PullConfig, branch: string): string | undefined {
  return (config.get(`branch.${branch}.rebase`) ?? config.get("pull.rebase"))?.toLowerCase();
}

/** Whether a pull of `branch` can make a merge commit, going by `config`. */
export function makesMergeCommit(config: PullConfig, branch: string): boolean {
  return !rebases(config, branch) && config.get("pull.ff")?.toLowerCase() !== "only";
}

/** Whether a pull of `branch` rebases, going by `config`: as in `git pull`, `pull.ff=only` wins. */
export function rebases(config: PullConfig, branch: string): boolean {
  const rebase = rebaseSetting(config, branch);
  return (
    config.get("pull.ff")?.toLowerCase() !== "only" && rebase !== undefined && !FALSE.has(rebase)
  );
}
