import { spawn } from "node:child_process";
// Windows paths, wherever it runs (the tests).
import { win32 } from "node:path";

import { app } from "electron";

/** The Squirrel package's id (see forge.config.ts) and the executable's name, without `.exe`. */
const PACKAGE_ID = "Gitto";
const EXECUTABLE = "Gitto";

/**
 * On Windows, sees to what Squirrel runs Gitto for when it installs, updates or uninstalls it, and
 * returns whether it was one of those runs, which do nothing else: Gitto quits once it's done.
 * Electron's executable says it handles these itself, so Squirrel doesn't make or remove the
 * Start menu and desktop shortcuts; they're named after its product name, Gitto.
 */
export function handleSquirrelEvent(): boolean {
  if (process.platform !== "win32" || !app.isPackaged) return false;
  // The ID Squirrel gives the shortcuts, so the running window is grouped with a pinned one, and
  // notifications are shown as Gitto's.
  app.setAppUserModelId(`com.squirrel.${PACKAGE_ID}.${EXECUTABLE}`);

  const exe = win32.basename(process.execPath);
  switch (process.argv[1]) {
    case "--squirrel-install":
    case "--squirrel-updated":
      return update(`--createShortcut=${exe}`);
    case "--squirrel-uninstall":
      return update(`--removeShortcut=${exe}`);
    case "--squirrel-obsolete":
      app.quit();
      return true;
    default:
      return false;
  }
}

/** Runs Squirrel's Update.exe, installed next to the version's folder, then quits. */
function update(arg: string) {
  const exe = win32.resolve(process.execPath, "..", "..", "Update.exe");
  spawn(exe, [arg], { detached: true })
    .on("error", () => app.quit())
    .on("close", () => app.quit());
  return true;
}
