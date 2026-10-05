import { EventEmitter, on } from "node:events";

import type { Updates } from "@gitto/system/server";
import type { UpdateChannel, UpdateState } from "@gitto/system/types";
import { app, autoUpdater, net } from "electron";

/** Where Gitto is released (see .github/workflows/release.yml). */
const REPOSITORY = "msvdaamen/gitto";
const CHECK_EVERY_MS = 60 * 60_000;
/** As the release workflow takes them: a pre-release part, if any, starts with a letter. */
const SEMVER =
  /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[A-Za-z][0-9A-Za-z-]*(\.[0-9A-Za-z-]+)*)?$/;

/** A nightly's version is `nightly` and when it was built, in minutes: 1.2.4-nightly29853462. */
export function channelOf(version: string): UpdateChannel {
  return prerelease(version).some((part) => /^nightly\d+$/.test(part)) ? "nightly" : "release";
}

/**
 * Where the channel's latest build is published, with the files the updaters read: the rolling
 * nightly pre-release, or the latest release, which GitHub never takes a pre-release for.
 */
export function releaseUrl(channel: UpdateChannel) {
  const releases = `https://github.com/${REPOSITORY}/releases`;
  return channel === "nightly" ? `${releases}/download/nightly` : `${releases}/latest/download`;
}

/** Compares two versions by semver's precedence: negative if `a` comes before `b`, positive if after. */
export function compareVersions(a: string, b: string): number {
  const [aCore, bCore] = [core(a), core(b)];
  for (let i = 0; i < 3; i++) {
    if (aCore[i] !== bCore[i]) return aCore[i]! - bCore[i]!;
  }
  const [aPre, bPre] = [prerelease(a), prerelease(b)];
  // A pre-release comes before its release.
  if (!aPre.length || !bPre.length) return bPre.length - aPre.length;
  for (let i = 0; i < Math.min(aPre.length, bPre.length); i++) {
    const order = compareIdentifiers(aPre[i]!, bPre[i]!);
    if (order) return order;
  }
  return aPre.length - bPre.length;
}

function core(version: string) {
  return version.split(/[-+]/, 1)[0]!.split(".").map(Number);
}

function prerelease(version: string) {
  const plain = version.split("+", 1)[0]!;
  const dash = plain.indexOf("-");
  return dash === -1 ? [] : plain.slice(dash + 1).split(".");
}

function compareIdentifiers(a: string, b: string) {
  const [aNumber, bNumber] = [/^\d+$/.test(a), /^\d+$/.test(b)];
  if (aNumber && bNumber) return Number(a) - Number(b);
  // Numbers come before words.
  if (aNumber || bNumber) return aNumber ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * Keeps Gitto up to date on Windows (Squirrel) and macOS, from its own channel: a nightly updates
 * to the next nightly, a release to the next release. Each release has an update.json saying its
 * version; once it's newer than this one, Electron's updater downloads it, and it's installed when
 * the user restarts into it, or the next time Gitto starts.
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
    const release = releaseUrl(this.state.channel);
    // Squirrel reads the release's RELEASES file. Squirrel.Mac asks the URL whether there's an
    // update, which this one always says there is; it's only asked once update.json says so.
    autoUpdater.setFeedURL({
      url:
        process.platform === "darwin" ? `${release}/update-darwin-${process.arch}.json` : release,
    });
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
      const response = await net.fetch(`${releaseUrl(this.state.channel)}/update.json`, {
        cache: "no-store",
      });
      if (!response.ok) throw new Error(`update.json: HTTP ${response.status}`);
      const { version } = (await response.json()) as { version?: unknown };
      if (typeof version !== "string" || !SEMVER.test(version)) {
        throw new Error(`update.json: invalid version ${JSON.stringify(version)}`);
      }
      if (compareVersions(version, this.state.version) <= 0 || this.state.update) return;
      this.setUpdate({ version, ready: false });
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
