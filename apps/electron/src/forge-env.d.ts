// Injected by @electron-forge/plugin-vite for the `main_window` renderer.
declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string | undefined;
declare const MAIN_WINDOW_VITE_NAME: string;

declare module "*?inline" {
  const dataUrl: string;
  export default dataUrl;
}

// Set by vite.main.config.ts from the SENTRY_DSN Gitto is built with; empty without one.
declare const SENTRY_DSN: string;
