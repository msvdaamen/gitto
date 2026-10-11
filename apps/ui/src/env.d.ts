interface Window {
  /** Exposed by the Electron preload script. Undefined when running in a regular browser. */
  electron?: {
    platform: string;
    versions: {
      electron: string;
      chrome: string;
      node: string;
    };
  };
}

interface ImportMetaEnv {
  /** Set by vite.config.ts: whether Gitto is built to report to Sentry. */
  readonly SENTRY: boolean;
}
