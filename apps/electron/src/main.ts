import { stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { RPC_CONNECT_CHANNEL } from "@gitto/rpc";
import { createMainRpcHandler } from "@gitto/rpc/main";
import { app, BrowserWindow, dialog, ipcMain, nativeImage, net, protocol, shell } from "electron";

import iconDataUrl from "../assets/icon.png?inline";
import { startBackend } from "./backend-process";
import { resolveRendererPath } from "./renderer-path";
import { traceEventLoopStalls } from "./trace";

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

// A second instance would contend for the same profile locks and take ~4s to show anything,
// so hand off to the running instance instead.
if (!app.requestSingleInstanceLock()) {
  app.exit(0);
}

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

/** Where the backend process finds the database and its migrations. */
function databasePaths() {
  return {
    database: join(app.getPath("userData"), "gitto.db"),
    // Packaged builds ship the migrations as an extra resource (see forge.config.ts).
    migrations: app.isPackaged
      ? join(process.resourcesPath, "migrations")
      : resolve(app.getAppPath(), "migrations"),
  };
}

async function selectFolder() {
  // The picker is opened from a click, so the focused window is the one that asked.
  const win = BrowserWindow.getFocusedWindow();
  const options: Electron.OpenDialogOptions = { properties: ["openDirectory"] };
  const result = await (win ? dialog.showOpenDialog(win, options) : dialog.showOpenDialog(options));
  return result.canceled ? null : (result.filePaths[0] ?? null);
}

/**
 * Serves the API. Only what needs the main process (native dialogs) is handled here; the rest,
 * everything that reads the disk or runs git, by the backend process, so none of it can keep this
 * one busy.
 */
function registerRpc() {
  const handler = createMainRpcHandler();
  const backend = startBackend(databasePaths());

  // Each renderer connection sends an end of two MessageChannels (see the preload): one for this
  // process, one for the backend.
  ipcMain.on(RPC_CONNECT_CHANNEL, (event) => {
    const [mainPort, backendPort] = event.ports;
    if (!mainPort || !backendPort) return;
    handler.upgrade(mainPort, { context: { selectFolder } });
    mainPort.start();
    backend.connect(backendPort);
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

// Not `await app.whenReady()`: top-level await in the ESM entry blocks Electron's startup.
app.on("ready", () => {
  if (process.env.GITTO_TRACE) traceEventLoopStalls("main");
  registerAppProtocol();
  registerRpc();
  createWindow();
});
