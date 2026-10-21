import { spawn } from "node:child_process";
import { userInfo } from "node:os";
import { delimiter } from "node:path";

/** Marks where the PATH starts and ends, among whatever the user's shell startup files print. */
const MARKER = "_GITTO_SHELL_PATH_";
/** How long the user's shell may take to start before Gitto goes on with the PATH it was given. */
const TIMEOUT_MS = 5000;

/**
 * Gives Gitto the PATH the user's terminal has. Opened from the Finder, the Dock or a desktop
 * launcher, an app only gets a minimal PATH (on macOS, `/usr/bin:/bin:/usr/sbin:/sbin`), without
 * what the user's shell startup files add, like Homebrew's `/opt/homebrew/bin`. Git would then be
 * the one in `/usr/bin` rather than the one they installed, and what git runs, like git-lfs, a
 * credential helper or gpg, wouldn't be found at all.
 *
 * Asks the user's login shell for its PATH and puts it ahead of the one Gitto was given, which is
 * kept as it was if the shell can't be asked. Not on Windows, where apps get the user's PATH.
 */
export async function usePathFromShell(): Promise<void> {
  if (process.platform === "win32") return;
  const shell = process.env.SHELL || userInfo().shell;
  if (!shell) return;
  const path = await readShellPath(shell);
  if (path) process.env.PATH = mergePaths(path, process.env.PATH ?? "");
}

/** The shell's PATH in what it printed, between the markers; `null` if there's none. */
export function parseShellPath(output: string): string | null {
  const start = output.indexOf(MARKER);
  const end = output.lastIndexOf(MARKER);
  if (start === -1 || end <= start) return null;
  return output.slice(start + MARKER.length, end).trim() || null;
}

/** `first`'s directories, then those of `rest` that aren't in it. */
export function mergePaths(first: string, rest: string): string {
  const dirs = new Set(first.split(delimiter).filter(Boolean));
  for (const dir of rest.split(delimiter)) if (dir) dirs.add(dir);
  return [...dirs].join(delimiter);
}

/**
 * Runs `shell` as an interactive login shell, so it reads the same startup files as a terminal,
 * and resolves to the PATH it has; `null` if it fails or takes too long. It runs in a session of
 * its own, so an interactive shell can't take over the terminal Gitto may have been started from.
 */
function readShellPath(shell: string): Promise<string | null> {
  return new Promise((resolve) => {
    // printenv rather than $PATH, which fish has as a list.
    const command = `printf '%s' ${MARKER}; printenv PATH; printf '%s' ${MARKER}`;
    const child = spawn(shell, ["-ilc", command], {
      // Keeps oh-my-zsh from asking to update itself, which no one would answer.
      env: { ...process.env, DISABLE_AUTO_UPDATE: "true" },
      stdio: ["ignore", "pipe", "ignore"],
      detached: true,
    });
    const stdout: Buffer[] = [];
    const timer = setTimeout(() => {
      // With what the startup files started, in its group.
      try {
        if (child.pid) process.kill(-child.pid);
      } catch {
        // Already gone.
      }
      resolve(null);
    }, TIMEOUT_MS);
    child.stdout?.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.on("error", () => {
      clearTimeout(timer);
      resolve(null);
    });
    child.on("close", () => {
      clearTimeout(timer);
      resolve(parseShellPath(Buffer.concat(stdout).toString("utf8")));
    });
  });
}
