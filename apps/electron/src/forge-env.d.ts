// Injected by @electron-forge/plugin-vite for the `main_window` renderer.
declare const MAIN_WINDOW_VITE_DEV_SERVER_URL: string | undefined;
declare const MAIN_WINDOW_VITE_NAME: string;

declare module "*?inline" {
  const dataUrl: string;
  export default dataUrl;
}

/** Where to send error reports, from SENTRY_DSN when it's built (see vite.main.config.ts); or "". */
declare const SENTRY_DSN: string;
