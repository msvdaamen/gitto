import { EventEmitter, on } from "node:events";

import { channelOf, compareVersions, isPrerelease, SEMVER } from "@gitto/release/version";
import type { Updates } from "@gitto/system/server";
import type { UpdateChannel, UpdateState } from "@gitto/system/types";
import { app, autoUpdater, net } from "electron";

/** Where Gitto is released (see .github/workflows/release.yml). */
const REPOSITORY = "msvdaamen/gitto";
/**
 * The newest releases, nightlies among them: the release workflow keeps the last 30 nightlies.
 * Unauthenticated, GitHub answers 60 requests an hour, far more than an hourly check asks.
 */
export const RELEASES_API = `https://api.github.com/repos/${REPOSITORY}/releases?per_page=50`;
const CHECK_EVERY_MS = 60 * 60_000;

/** Where a version's files are published, tagged `v<version>`, with those the updaters read. */
export function releaseUrl(version: string) {
  return `https://github.com/${REPOSITORY}/releases/download/v${version}`;
}

/**
 * The newest version on the channel among the releases GitHub lists: a nightly, or a release,
 * leaving out pre-releases like 1.3.0-beta.1, which are only installed by hand.
 */
export function newestOn(channel: UpdateChannel, releases: unknown): string | null {
  if (!Array.isArray(releases)) throw new Error("releases: not a list");
  return (
    releases
      .flatMap((release: { tag_name?: unknown; draft?: unknown }) => {
        const tag = release?.tag_name;
        if (release?.draft || typeof tag !== "string" || !tag.startsWith("v")) return [];
        const version = tag.slice(1);
        if (!SEMVER.test(version) || channelOf(version) !== channel) return [];
        return channel === "release" && isPrerelease(version) ? [] : [version];
      })
      .toSorted(compareVersions)
      .at(-1) ?? null
  );
}

/**
 * Keeps Gitto up to date on Windows (Squirrel) and macOS, from its own channel: a nightly updates
 * to the next nightly, a release to the next release. Once GitHub lists a newer one, Electron's
 * updater downloads it from its release, and it's installed when the user restarts into it, or the
 * next time Gitto starts.
 *
 * Linux has no updater: Gitto's installed there by pacman, from the AUR, which updates it.
 */
export class Updater implements Updates {
  private readonly events = new EventEmitter();
  private state: UpdateState;

  constructor() {
    const version = app.getVersion();
    this.state = { version, channel: channelOf(version), update: null };
  }

  /** Checks for an update now and every hour, where Gitto updates itself. */
  start() {
    if (!this.updatesItself()) return;
    autoUpdater.on("update-downloaded", () => {
      if (this.state.update) this.setUpdate({ ...this.state.update, ready: true });
    });
    // Squirrel didn't find it after all (e.g. the release was being replaced), or the download
    // failed: the next check tries again.
    autoUpdater.on("update-not-available", () => this.setUpdate(null));
    autoUpdater.on("error", (error) => {
      console.error("[update]", error);
      this.setUpdate(null);
    });

    // On the first run after installing, Squirrel's still busy, and an update would fail.
    if (!process.argv.includes("--squirrel-firstrun")) void this.check();
    setInterval(() => void this.check(), CHECK_EVERY_MS).unref();
  }

  async check() {
    // One update at a time: a newer one after it is found once Gitto has restarted into it.
    if (this.state.update) return;
    try {
      const response = await net.fetch(RELEASES_API, {
        cache: "no-store",
        headers: { Accept: "application/vnd.github+json" },
      });
      if (!response.ok) throw new Error(`releases: HTTP ${response.status}`);
      const version = newestOn(this.state.channel, await response.json());
      if (!version || compareVersions(version, this.state.version) <= 0 || this.state.update)
        return;
      this.setUpdate({ version, ready: false });
      // Squirrel reads the release's RELEASES file. Squirrel.Mac asks the URL whether there's an
      // update, which this one always says there is: it points to the release's zip.
      const release = releaseUrl(version);
      autoUpdater.setFeedURL({
        url:
          process.platform === "darwin" ? `${release}/update-darwin-${process.arch}.json` : release,
      });
      autoUpdater.checkForUpdates();
    } catch (error) {
      console.error("[update] Couldn't check for an update", error);
    }
  }

  async *watch(signal?: AbortSignal) {
    // Listening first, so nothing that changes meanwhile is missed.
    const changes = on(this.events, "change", { signal });
    try {
      yield this.state;
      for await (const [state] of changes) yield state as UpdateState;
    } catch (error) {
      if (!signal?.aborted) throw error;
    } finally {
      await changes.return?.();
    }
  }

  install() {
    if (this.state.update?.ready) autoUpdater.quitAndInstall();
  }

  private updatesItself() {
    return (
      app.isPackaged &&
      (process.platform === "win32" || process.platform === "darwin") &&
      // A build of one's own, not CI's, isn't replaced by a release.
      this.state.version !== "0.0.0"
    );
  }

  private setUpdate(update: UpdateState["update"]) {
    this.state = { ...this.state, update };
    this.events.emit("change", this.state);
  }
}
