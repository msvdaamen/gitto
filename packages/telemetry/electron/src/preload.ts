// Exposes IPC to the renderer's Sentry SDK, which otherwise falls back to fetch over the
// sentry-ipc:// protocol, and its browser tracing records every envelope it sends as a span.
import "@sentry/electron/preload";
