import { join } from "node:path";

import { app, BrowserWindow, dialog, utilityProcess, type MessagePortMain } from "electron";

import { BACKEND_ENV } from "./backend-env";

/** A backend that stops this soon after starting isn't started again: it would just keep failing. */
const CRASH_LOOP_MS = 10_000;

/**
 * Runs the backend (see backend.ts) as a utility process, and starts it again if it stops by
 * itself. Call after the app is ready.
 */
export function startBackend(paths: { database: string; migrations: string }) {
  let quitting = false;
  let child = fork();

  function fork() {
    const started = performance.now();
    // Ports to hand over once the process runs.
    let waiting: MessagePortMain[] | undefined = [];
    const next = utilityProcess.fork(join(import.meta.dirname, "backend.js"), [], {
      serviceName: "Gitto Backend",
      env: {
        ...process.env,
        [BACKEND_ENV.database]: paths.database,
        [BACKEND_ENV.migrations]: paths.migrations,
      },
    });
    next.once("spawn", () => {
      for (const port of waiting ?? []) next.postMessage(null, [port]);
      waiting = undefined;
    });
    next.once("exit", (code) => {
      if (quitting) return;
      console.error(`[main] the backend stopped with code ${code}`);
      if (performance.now() - started < CRASH_LOOP_MS) {
        dialog.showErrorBox("Gitto can't start", `Its backend process stopped (code ${code}).`);
        app.quit();
        return;
      }
      child = fork();
      // Their connections went with the old backend; reloading makes new ones.
      for (const win of BrowserWindow.getAllWindows()) win.webContents.reload();
    });
    return {
      process: next,
      connect: (port: MessagePortMain) => {
        if (waiting) waiting.push(port);
        else next.postMessage(null, [port]);
      },
    };
  }

  app.on("will-quit", () => {
    quitting = true;
    child.process.kill();
  });

  return {
    /** Hands a renderer's MessagePort to the backend, which serves the API over it. */
    connect(port: MessagePortMain) {
      child.connect(port);
    },
  };
}
