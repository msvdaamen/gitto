import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { readTelemetrySetting, TelemetrySetting } from "./telemetry";

let dir: string;
let file: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "gitto-telemetry-"));
  file = join(dir, "settings.json");
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
  vi.restoreAllMocks();
});

describe("the telemetry setting", () => {
  it("is on until it's turned off", () => {
    expect(readTelemetrySetting(file)).toBe(true);
  });

  it("is on when the settings can't be read", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    writeFileSync(file, "{");

    expect(readTelemetrySetting(file)).toBe(true);
    expect(console.error).toHaveBeenCalled();
  });

  it("stops sending at once when it's turned off, and is remembered", async () => {
    const stop = vi.fn(async () => {});
    const setting = new TelemetrySetting(file, true, true, true, stop);

    expect(await setting.setEnabled(false)).toEqual({
      available: true,
      enabled: false,
      sending: false,
    });
    expect(stop).toHaveBeenCalledOnce();
    expect(JSON.parse(readFileSync(file, "utf8"))).toEqual({ telemetry: false });
    expect(readTelemetrySetting(file)).toBe(false);
  });

  it("only starts sending once Gitto restarts when it's turned on", async () => {
    writeFileSync(file, JSON.stringify({ telemetry: false }));
    const stop = vi.fn(async () => {});
    const setting = new TelemetrySetting(file, true, false, false, stop);

    expect(await setting.setEnabled(true)).toEqual({
      available: true,
      enabled: true,
      sending: false,
    });
    expect(stop).not.toHaveBeenCalled();
    expect(readTelemetrySetting(file)).toBe(true);
  });
});
