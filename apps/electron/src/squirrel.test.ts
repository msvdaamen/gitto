import { EventEmitter } from "node:events";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { app, spawn } = vi.hoisted(() => ({
  app: { isPackaged: true, quit: vi.fn(), setAppUserModelId: vi.fn() },
  spawn: vi.fn(),
}));
vi.mock("electron", () => ({ app }));
vi.mock("node:child_process", () => ({ spawn }));

import { handleSquirrelEvent } from "./squirrel";

const EXE = String.raw`C:\Users\me\AppData\Local\Gitto\app-1.2.3\Gitto.exe`;
const { platform, argv, execPath } = process;
let child: EventEmitter;

function run(flag?: string, os: NodeJS.Platform = "win32") {
  Object.defineProperty(process, "platform", { value: os });
  process.argv = flag ? [EXE, flag] : [EXE];
  process.execPath = EXE;
  return handleSquirrelEvent();
}

beforeEach(() => {
  child = new EventEmitter();
  spawn.mockReturnValue(child);
});

afterEach(() => {
  Object.defineProperty(process, "platform", { value: platform });
  process.argv = argv;
  process.execPath = execPath;
  app.isPackaged = true;
  vi.clearAllMocks();
});

describe("Squirrel events", () => {
  it("makes the shortcuts on install and update, then quits once Update.exe is done", () => {
    for (const flag of ["--squirrel-install", "--squirrel-updated"]) {
      expect(run(flag)).toBe(true);
      expect(spawn).toHaveBeenLastCalledWith(
        String.raw`C:\Users\me\AppData\Local\Gitto\Update.exe`,
        ["--createShortcut=Gitto.exe"],
        { detached: true },
      );
    }
    expect(app.quit).not.toHaveBeenCalled();
    child.emit("close", 0);
    expect(app.quit).toHaveBeenCalled();
  });

  it("removes them on uninstall", () => {
    expect(run("--squirrel-uninstall")).toBe(true);
    expect(spawn).toHaveBeenCalledWith(expect.anything(), ["--removeShortcut=Gitto.exe"], {
      detached: true,
    });
  });

  it("quits when Update.exe can't be run", () => {
    run("--squirrel-install");
    child.emit("error", new Error("ENOENT"));
    expect(app.quit).toHaveBeenCalled();
  });

  it("quits at once when the version is obsolete", () => {
    expect(run("--squirrel-obsolete")).toBe(true);
    expect(spawn).not.toHaveBeenCalled();
    expect(app.quit).toHaveBeenCalled();
  });

  it("lets other runs go ahead, with the shortcuts' app ID", () => {
    expect(run()).toBe(false);
    expect(run("--squirrel-firstrun")).toBe(false);
    expect(app.setAppUserModelId).toHaveBeenCalledWith("com.squirrel.Gitto.Gitto");
    expect(spawn).not.toHaveBeenCalled();
    expect(app.quit).not.toHaveBeenCalled();
  });

  it("does nothing off Windows, or unpackaged", () => {
    expect(run("--squirrel-install", "linux")).toBe(false);
    app.isPackaged = false;
    expect(run("--squirrel-install")).toBe(false);
    expect(app.setAppUserModelId).not.toHaveBeenCalled();
    expect(spawn).not.toHaveBeenCalled();
  });
});
