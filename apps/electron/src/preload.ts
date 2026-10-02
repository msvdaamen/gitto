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

// MessagePorts can't cross the contextBridge, so the renderer posts its RPC ports to the window and
// this forwards them to the main process. The origin check keeps embedded frames from connecting.
window.addEventListener("message", (event) => {
  if (event.data !== RPC_CONNECT_CHANNEL || event.origin !== window.location.origin) return;
  ipcRenderer.postMessage(RPC_CONNECT_CHANNEL, null, [...event.ports]);
});
