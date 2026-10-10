// Lets the renderer's Sentry send what it reports through the main process (see src/sentry.ts).
// Sentry would add it by itself, but can't find its file once the main process is bundled.
import "@sentry/electron/preload";
import { RPC_CONNECT_CHANNEL } from "@gitto/rpc";
import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("electron", {
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    node: process.versions.node,
  },
});

// MessagePorts can't cross the contextBridge, so the renderer posts its RPC port to the window and
// this forwards it to the main process. The origin check keeps embedded frames from connecting.
window.addEventListener("message", (event) => {
  if (event.data !== RPC_CONNECT_CHANNEL || event.origin !== window.location.origin) return;
  ipcRenderer.postMessage(RPC_CONNECT_CHANNEL, null, [...event.ports]);
});
