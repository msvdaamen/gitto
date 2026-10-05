import { execFile } from "node:child_process";

import type { GitInstall } from "../features/version/schema";
import { UnsupportedGitError } from "./errors";

/** The oldest git Gitto works with: it uses options and output that older ones don't have. */
export const MIN_GIT_VERSION = "2.41";

/**
 * The version in `git version`'s output, e.g. `2.45.1` from `git version 2.45.1.windows.1` or
 * `2.39.3` from `git version 2.39.3 (Apple Git-146)`; `null` if there's none.
 */
export function parseGitVersion(output: string): string | null {
  return /^git version (\d+\.\d+(?:\.\d+)?)/.exec(output.trim())?.[1] ?? null;
}

/** Whether `version` (e.g. `2.39.3`) is `MIN_GIT_VERSION` or newer. */
export function isSupportedVersion(version: string): boolean {
  const [major = 0, minor = 0] = version.split(".").map(Number);
  const [minMajor = 0, minMinor = 0] = MIN_GIT_VERSION.split(".").map(Number);
  return major !== minMajor ? major > minMajor : minor >= minMinor;
}

/** Runs `git version`; resolves to its output, or `null` if git couldn't be run. */
function readGitVersion(): Promise<string | null> {
  return new Promise((resolve) => {
    execFile("git", ["version"], { env: { ...process.env, LC_ALL: "C" } }, (error, stdout) =>
      resolve(error ? null : stdout),
    );
  });
}

/**
 * Checks that the installed git is one Gitto works with. A supported git is only checked once per
 * run of the app; one that isn't is checked again every time, so Gitto works again as soon as the
 * user has updated git, without restarting.
 */
export class GitVersion {
  private checked: Promise<GitInstall> | undefined;

  constructor(private readonly read: () => Promise<string | null> = readGitVersion) {}

  check(): Promise<GitInstall> {
    this.checked ??= this.read().then((output) => {
      const install = toInstall(output);
      // Only a supported git is kept; the next call checks again.
      if (!install.supported) this.checked = undefined;
      return install;
    });
    return this.checked;
  }

  /** Rejects with an `UnsupportedGitError` if the installed git isn't supported. */
  async require(): Promise<void> {
    const install = await this.check();
    if (!install.supported) throw new UnsupportedGitError(install);
  }
}

function toInstall(output: string | null): GitInstall {
  if (output === null) return { version: null, required: MIN_GIT_VERSION, supported: false };
  const version = parseGitVersion(output);
  // A build that words it differently is given the benefit of the doubt.
  if (version === null)
    return { version: output.trim(), required: MIN_GIT_VERSION, supported: true };
  return { version, required: MIN_GIT_VERSION, supported: isSupportedVersion(version) };
}
