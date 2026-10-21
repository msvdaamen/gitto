import { EventEmitter } from "node:events";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { spawn } = vi.hoisted(() => ({ spawn: vi.fn() }));
vi.mock("node:child_process", () => ({ spawn }));

import { mergePaths, parseShellPath, usePathFromShell } from "./shell-path";

const MARKER = "_GITTO_SHELL_PATH_";
const { platform } = process;
const { PATH, SHELL } = process.env;
let child: EventEmitter & { stdout: EventEmitter; pid: number };

function prints(output: string) {
  setImmediate(() => {
    child.stdout.emit("data", Buffer.from(output));
    child.emit("close", 0);
  });
}

beforeEach(() => {
  child = Object.assign(new EventEmitter(), { stdout: new EventEmitter(), pid: 4242 });
  spawn.mockReturnValue(child);
  Object.defineProperty(process, "platform", { value: "darwin" });
  process.env.PATH = "/usr/bin:/bin:/usr/sbin:/sbin";
  process.env.SHELL = "/bin/zsh";
});

afterEach(() => {
  Object.defineProperty(process, "platform", { value: platform });
  process.env.PATH = PATH;
  process.env.SHELL = SHELL;
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("the PATH from the user's shell", () => {
  it("is put ahead of the one Gitto was given", async () => {
    prints(`${MARKER}/opt/homebrew/bin:/usr/bin:/bin\n${MARKER}`);
    await usePathFromShell();
    expect(process.env.PATH).toBe("/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin");
    expect(spawn).toHaveBeenCalledWith(
      "/bin/zsh",
      ["-ilc", expect.stringContaining("printenv PATH")],
      expect.objectContaining({ detached: true }),
    );
  });

  it("is found among what the startup files print", () => {
    expect(parseShellPath(`Welcome!\n${MARKER}/a:/b\n${MARKER}\nbye`)).toBe("/a:/b");
  });

  it("leaves the PATH as it was when the shell prints none", async () => {
    prints("zsh: command not found: printenv\n");
    await usePathFromShell();
    expect(process.env.PATH).toBe("/usr/bin:/bin:/usr/sbin:/sbin");
  });

  it("leaves the PATH as it was when the shell can't be run", async () => {
    setImmediate(() => child.emit("error", new Error("ENOENT")));
    await usePathFromShell();
    expect(process.env.PATH).toBe("/usr/bin:/bin:/usr/sbin:/sbin");
  });

  it("stops waiting for a shell that doesn't finish", async () => {
    vi.useFakeTimers();
    const kill = vi.spyOn(process, "kill").mockReturnValue(true);
    const done = usePathFromShell();
    await vi.advanceTimersByTimeAsync(5000);
    await done;
    expect(kill).toHaveBeenCalledWith(-4242);
    expect(process.env.PATH).toBe("/usr/bin:/bin:/usr/sbin:/sbin");
    kill.mockRestore();
  });

  it("isn't asked for on Windows", async () => {
    Object.defineProperty(process, "platform", { value: "win32" });
    await usePathFromShell();
    expect(spawn).not.toHaveBeenCalled();
  });
});

describe("merging PATHs", () => {
  it("keeps each directory once, in order", () => {
    expect(mergePaths("/a:/b::/c", "/b:/d:/a:/e")).toBe("/a:/b:/c:/d:/e");
  });
});
