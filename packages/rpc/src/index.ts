/**
 * IPC channel (and window message) used to hand the renderer's MessagePorts to the main process:
 * the one for the main process first, then the one for the backend process.
 */
export const RPC_CONNECT_CHANNEL = "gitto:rpc-connect";
