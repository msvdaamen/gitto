import { homedir } from "node:os";

import { runGit } from "../../core/runner";

/** `user.name` from git's global config; `null` if it isn't set, or git couldn't be run. */
export async function getUserName(): Promise<string | null> {
  // Run in the home folder: it's the global config that's asked for, whatever folder git is in.
  const name = await runGit(homedir(), ["config", "--global", "user.name"]).then(
    (output) => output.trim(),
    () => "",
  );
  return name || null;
}
