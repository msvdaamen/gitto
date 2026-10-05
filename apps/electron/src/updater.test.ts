import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { app, autoUpdater, net } = await vi.hoisted(async () => {
  const { EventEmitter } = await import("node:events");
  return {
    app: { isPackaged: true, getVersion: vi.fn() },
    autoUpdater: Object.assign(new EventEmitter(), {
      setFeedURL: vi.fn(),
      checkForUpdates: vi.fn(),
      quitAndInstall: vi.fn(),
    }),
    net: { fetch: vi.fn() },
  };
});
vi.mock("electron", () => ({ app, autoUpdater, net }));

import { channelOf, compareVersions, releaseUrl, Updater } from "./updater";

const NIGHTLY = "https://github.com/msvdaamen/gitto/releases/download/nightly";
const LATEST = "https://github.com/msvdaamen/gitto/releases/latest/download";
const { platform, arch, argv } = process;

describe("versions", () => {
  it("are on the nightly channel with a nightly part, else on the release channel", () => {
    expect(channelOf("1.2.4-nightly29853462")).toBe("nightly");
    expect(channelOf("1.2.3")).toBe("release");
    expect(channelOf("1.3.0-beta.1")).toBe("release");
    expect(channelOf("0.0.0")).toBe("release");
  });

  it("are updated from the rolling nightly, or the latest release", () => {
    expect(releaseUrl("nightly")).toBe(NIGHTLY);
    expect(releaseUrl("release")).toBe(LATEST);
  });

  it("are ordered as semver orders them", () => {
    const ordered = [
      "0.9.9",
      "1.2.3",
      "1.2.4-nightly29853462",
      "1.2.4-nightly29853463",
      "1.2.4-nightly99930239",
      "1.2.4",
      "1.2.10",
      "1.3.0-alpha",
      "1.3.0-alpha.1",
      "1.3.0-alpha.beta",
      "1.3.0-beta.2",
      "1.3.0-beta.11",
      "1.3.0-nightly29853462",
      "1.3.0-rc.1",
      "1.3.0-rc.2",
      "1.3.0",
    ];
    for (const [i, a] of ordered.entries()) {
      for (const [j, b] of ordered.entries()) {
        expect(Math.sign(compareVersions(a, b)), `${a} vs ${b}`).toBe(Math.sign(i - j));
      }
    }
    expect(compareVersions("1.2.3+build.1", "1.2.3")).toBe(0);
  });
});

function updater(version: string, os: NodeJS.Platform = "win32", cpu = "x64") {
  Object.defineProperty(process, "platform", { value: os });
  Object.defineProperty(process, "arch", { value: cpu });
  app.getVersion.mockReturnValue(version);
  return new Updater();
}

function published(version: unknown) {
  net.fetch.mockImplementation(async () => Response.json({ version }));
}

/** The states it emits from now on. */
function watch(updates: Updater) {
  const controller = new AbortController();
  const states: unknown[] = [];
  void (async () => {
    for await (const state of updates.watch(controller.signal)) states.push(state);
  })();
  return { states, stop: () => controller.abort() };
}

describe("the updater", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    Object.defineProperty(process, "platform", { value: platform });
    Object.defineProperty(process, "arch", { value: arch });
    process.argv = argv;
    app.isPackaged = true;
    autoUpdater.removeAllListeners();
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it("checks a nightly's channel, and downloads a newer nightly", async () => {
    published("1.2.4-nightly29854900");
    const updates = updater("1.2.4-nightly29853462");
    const { states, stop } = watch(updates);
    updates.start();

    expect(autoUpdater.setFeedURL).toHaveBeenCalledWith({ url: NIGHTLY });
    expect(net.fetch).toHaveBeenCalledWith(`${NIGHTLY}/update.json`, { cache: "no-store" });
    await vi.waitFor(() => expect(autoUpdater.checkForUpdates).toHaveBeenCalled());

    autoUpdater.emit("update-downloaded");
    await vi.waitFor(() => expect(states).toHaveLength(3));
    expect(states).toEqual([
      { version: "1.2.4-nightly29853462", channel: "nightly", update: null },
      {
        version: "1.2.4-nightly29853462",
        channel: "nightly",
        update: { version: "1.2.4-nightly29854900", ready: false },
      },
      {
        version: "1.2.4-nightly29853462",
        channel: "nightly",
        update: { version: "1.2.4-nightly29854900", ready: true },
      },
    ]);

    updates.install();
    expect(autoUpdater.quitAndInstall).toHaveBeenCalled();
    stop();
  });

  it("checks a release's channel, and asks Squirrel.Mac for the zip on macOS", async () => {
    published("1.2.4");
    const updates = updater("1.2.3", "darwin", "arm64");
    updates.start();

    expect(autoUpdater.setFeedURL).toHaveBeenCalledWith({
      url: `${LATEST}/update-darwin-arm64.json`,
    });
    expect(net.fetch).toHaveBeenCalledWith(`${LATEST}/update.json`, { cache: "no-store" });
    await vi.waitFor(() => expect(autoUpdater.checkForUpdates).toHaveBeenCalled());
  });

  it("downloads nothing that isn't newer", async () => {
    const responses = [
      ...["1.2.3", "1.2.2", "1.2.3-rc.1", "not a version"].map((version) =>
        Response.json({ version }),
      ),
      new Response("Not found", { status: 404 }),
    ];
    for (const response of responses) net.fetch.mockResolvedValueOnce(response);
    net.fetch.mockRejectedValueOnce(new Error("offline"));
    await Promise.all(Array.from({ length: 6 }, () => updater("1.2.3").check()));
    expect(net.fetch).toHaveBeenCalledTimes(6);

    expect(autoUpdater.checkForUpdates).not.toHaveBeenCalled();
  });

  it("downloads one update at a time, and tries again after it failed", async () => {
    published("1.2.4");
    const updates = updater("1.2.3");
    updates.start();
    await vi.waitFor(() => expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1));

    await updates.check();
    expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(1);
    // Nothing's ready to install.
    updates.install();
    expect(autoUpdater.quitAndInstall).not.toHaveBeenCalled();

    autoUpdater.emit("error", new Error("download failed"));
    await updates.check();
    expect(autoUpdater.checkForUpdates).toHaveBeenCalledTimes(2);
  });

  it("doesn't update on Linux, unpackaged, or a build at 0.0.0", () => {
    updater("1.2.3", "linux").start();
    app.isPackaged = false;
    updater("1.2.3").start();
    app.isPackaged = true;
    updater("0.0.0").start();

    expect(autoUpdater.setFeedURL).not.toHaveBeenCalled();
    expect(net.fetch).not.toHaveBeenCalled();
  });

  it("waits for the next check on the first run after Squirrel installed it", () => {
    process.argv = ["Gitto.exe", "--squirrel-firstrun"];
    updater("1.2.3").start();

    expect(autoUpdater.setFeedURL).toHaveBeenCalled();
    expect(net.fetch).not.toHaveBeenCalled();
  });
});
