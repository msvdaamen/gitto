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

import { newestOn, RELEASES_API, releaseUrl, Updater } from "./updater";

const DOWNLOAD = "https://github.com/msvdaamen/gitto/releases/download";
const { platform, arch, argv } = process;

/** As GitHub lists them, the newest first. */
function releases(...tags: string[]) {
  return tags.map((tag_name) => ({ tag_name, draft: false }));
}

describe("releases", () => {
  it("are downloaded from their tag", () => {
    expect(releaseUrl("1.2.4-nightly29853462")).toBe(`${DOWNLOAD}/v1.2.4-nightly29853462`);
  });

  it("are updated to the channel's newest, leaving out other pre-releases and drafts", () => {
    const listed = [
      ...releases("v1.3.0-beta.1", "v1.2.4-nightly29854900", "nightly", "v1.2.4"),
      ...releases("v1.2.4-nightly29853462", "v1.2.3", "not a tag"),
      { tag_name: "v1.2.5-nightly29860000", draft: true },
      { tag_name: "v1.2.5", draft: true },
    ];
    expect(newestOn("nightly", listed)).toBe("1.2.4-nightly29854900");
    expect(newestOn("release", listed)).toBe("1.2.4");
    expect(newestOn("nightly", releases("v1.2.3"))).toBeNull();
    expect(() => newestOn("release", { message: "API rate limit exceeded" })).toThrow();
  });
});

function updater(version: string, os: NodeJS.Platform = "win32", cpu = "x64") {
  Object.defineProperty(process, "platform", { value: os });
  Object.defineProperty(process, "arch", { value: cpu });
  app.getVersion.mockReturnValue(version);
  return new Updater();
}

function published(...tags: string[]) {
  net.fetch.mockImplementation(async () => Response.json(releases(...tags)));
}

const FETCHED = [
  RELEASES_API,
  { cache: "no-store", headers: { Accept: "application/vnd.github+json" } },
] as const;

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
    published("v1.2.4", "v1.2.4-nightly29854900", "v1.2.4-nightly29853462");
    const updates = updater("1.2.4-nightly29853462");
    const { states, stop } = watch(updates);
    updates.start();

    expect(net.fetch).toHaveBeenCalledWith(...FETCHED);
    await vi.waitFor(() => expect(autoUpdater.checkForUpdates).toHaveBeenCalled());
    expect(autoUpdater.setFeedURL).toHaveBeenCalledWith({
      url: `${DOWNLOAD}/v1.2.4-nightly29854900`,
    });

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
    published("v1.3.0-beta.1", "v1.2.5-nightly29854900", "v1.2.4", "v1.2.3");
    const updates = updater("1.2.3", "darwin", "arm64");
    updates.start();

    expect(net.fetch).toHaveBeenCalledWith(...FETCHED);
    await vi.waitFor(() => expect(autoUpdater.checkForUpdates).toHaveBeenCalled());
    expect(autoUpdater.setFeedURL).toHaveBeenCalledWith({
      url: `${DOWNLOAD}/v1.2.4/update-darwin-arm64.json`,
    });
  });

  it("downloads nothing that isn't newer", async () => {
    const responses = [
      ...[["v1.2.3"], ["v1.2.2"], ["v1.2.4-rc.1"], ["v1.2.4-nightly29854900"], []].map((tags) =>
        Response.json(releases(...tags)),
      ),
      Response.json({ message: "API rate limit exceeded" }),
      new Response("Forbidden", { status: 403 }),
    ];
    for (const response of responses) net.fetch.mockResolvedValueOnce(response);
    net.fetch.mockRejectedValueOnce(new Error("offline"));
    await Promise.all(Array.from({ length: 8 }, () => updater("1.2.3").check()));
    expect(net.fetch).toHaveBeenCalledTimes(8);

    expect(autoUpdater.checkForUpdates).not.toHaveBeenCalled();
  });

  it("downloads one update at a time, and tries again after it failed", async () => {
    published("v1.2.4");
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
    const updates = updater("1.2.3");
    updates.start();

    expect(net.fetch).not.toHaveBeenCalled();
    // Listening, for the next check.
    expect(autoUpdater.listenerCount("update-downloaded")).toBe(1);
  });
});
