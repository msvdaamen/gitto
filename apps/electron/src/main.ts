import { stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { createDb } from "@gitto/db";
import { RPC_CONNECT_CHANNEL } from "@gitto/rpc";
import { createContainer, createRpcHandler } from "@gitto/rpc/server";
import { app, BrowserWindow, dialog, ipcMain, nativeImage, net, protocol, shell } from "electron";

import iconDataUrl from "../assets/icon.png?inline";
import { resolveRendererPath } from "./renderer-path";
import { initSentry, instrumentRpcCall } from "./sentry";
import { handleSquirrelEvent } from "./squirrel";
import { Updater } from "./updater";
import { Changes, readChangelog } from "./whats-new";

// Production builds are served from a custom protocol instead of file:// so the UI can use
// regular browser history routing (e.g. app://gitto/about).
const APP_SCHEME = "app";
const APP_URL = `${APP_SCHEME}://gitto/`;
const RENDERER_DIR = join(import.meta.dirname, "../renderer", MAIN_WINDOW_VITE_NAME);

// Keep dev runs out of the installed app's profile; sharing it (and its locks) with a running
// Gitto stalls startup by seconds.
if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
  app.setPath("userData", `${app.getPath("userData")}-dev`);
}

// Squirrel's runs while installing, updating or uninstalling only see to the shortcuts. They go
// ahead with Gitto running.
const squirrelRun = handleSquirrelEvent();

// A second instance would contend for the same profile locks and take ~4s to show anything,
// so hand off to the running instance instead.
if (!squirrelRun && !app.requestSingleInstanceLock()) {
  app.exit(0);
}

if (!squirrelRun) initSentry();

app.on("second-instance", () => {
  const win = BrowserWindow.getAllWindows()[0];
  if (!win) return;
  if (win.isMinimized()) win.restore();
  win.focus();
});

protocol.registerSchemesAsPrivileged([
  {
    scheme: APP_SCHEME,
    privileges: { standard: true, secure: true, supportFetchAPI: true },
  },
]);

function registerAppProtocol() {
  protocol.handle(APP_SCHEME, async (request) => {
    const { pathname } = new URL(request.url);
    const filePath = resolveRendererPath(RENDERER_DIR, pathname);
    if (!filePath) return new Response("Not found", { status: 404 });

    const isFile = await stat(filePath).then(
      (s) => s.isFile(),
      () => false,
    );
    if (isFile) return net.fetch(pathToFileURL(filePath).href);

    // Fall back to index.html for page navigations so client-side routes work on reload.
    if (request.headers.get("accept")?.includes("text/html")) {
      return net.fetch(pathToFileURL(join(RENDERER_DIR, "index.html")).href);
    }
    return new Response("Not found", { status: 404 });
  });
}

function openDatabase() {
  // Packaged builds ship the migrations as an extra resource (see forge.config.ts).
  const migrationsFolder = app.isPackaged
    ? join(process.resourcesPath, "migrations")
    : resolve(app.getAppPath(), "migrations");
  const db = createDb(join(app.getPath("userData"), "gitto.db"), migrationsFolder);
  app.on("will-quit", () => db.$client.close());
  return db;
}

function openChangelog() {
  // Packaged builds ship it as an extra resource (see forge.config.ts); in development, it's read
  // from apps/electron if one's been made there.
  const path = app.isPackaged
    ? join(process.resourcesPath, "changelog.json")
    : resolve(app.getAppPath(), "changelog.json");
  return new Changes(
    app.getVersion(),
    readChangelog(path),
    join(app.getPath("userData"), "last-seen-version"),
  );
}

async function selectFolder() {
  // The picker is opened from a click, so the focused window is the one that asked.
  const win = BrowserWindow.getFocusedWindow();
  const options: Electron.OpenDialogOptions = { properties: ["openDirectory"] };
  const result = await (win ? dialog.showOpenDialog(win, options) : dialog.showOpenDialog(options));
  return result.canceled ? null : (result.filePaths[0] ?? null);
}

function registerRpc(updates: Updater) {
  const handler = createRpcHandler({ instrument: instrumentRpcCall });
  const context = createContainer(openDatabase(), {
    selectFolder,
    updates,
    changelog: openChangelog(),
  });

  // Each renderer connection sends one end of a MessageChannel (see the preload).
  ipcMain.on(RPC_CONNECT_CHANNEL, (event) => {
    const [port] = event.ports;
    if (!port) return;
    handler.upgrade(port, { context });
    port.start();
  });
}

function createWindow() {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    show: false,
    // Window/taskbar icon on Linux and Windows; macOS uses the bundle icon instead.
    icon: nativeImage.createFromDataURL(iconDataUrl),
    webPreferences: {
      preload: join(import.meta.dirname, "preload.cjs"),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
    },
  });

  win.once("ready-to-show", () => win.show());

  // Open external links in the default browser instead of a new Electron window.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith("https:") || url.startsWith("http:")) {
      void shell.openExternal(url);
    }
    return { action: "deny" };
  });

  if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
    void win.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
    win.webContents.openDevTools({ mode: "detach" });
  } else {
    void win.loadURL(APP_URL);
  }
}

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow();
});

/**
 * With GITTO_TRACE=1, logs whenever something keeps the main process busy long enough to stall the
 * UI: it routes the renderer's input and IPC, so while it's blocked, the window can't respond.
 */
function traceEventLoopStalls() {
  const INTERVAL_MS = 20;
  const STALL_MS = 50;
  let last = performance.now();
  setInterval(() => {
    const now = performance.now();
    const stall = now - last - INTERVAL_MS;
    if (stall >= STALL_MS) console.log(`[main] event loop blocked for ${stall.toFixed(0)}ms`);
    last = now;
  }, INTERVAL_MS).unref();
}

// Not `await app.whenReady()`: top-level await in the ESM entry blocks Electron's startup.
app.on("ready", () => {
  if (squirrelRun) return;
  if (process.env.GITTO_TRACE) traceEventLoopStalls();
  registerAppProtocol();
  const updater = new Updater();
  registerRpc(updater);
  createWindow();
  updater.start();
});
