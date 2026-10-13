import { describe, expect, it } from "vitest";

import { scrubber } from "./scrub";

describe("scrubbing a report", () => {
  it("shortens paths in the home folder to ~, wherever they are", () => {
    const scrub = scrubber("/Users/mischa");
    const event = scrub({
      message: "fatal: not a git repository: /Users/mischa/code/gitto",
      exception: { values: [{ stacktrace: { frames: [{ filename: "/Users/mischa/a.js" }] } }] },
      breadcrumbs: [{ message: "opened /Users/Mischa/code/gitto", data: { count: 2 } }],
    });

    expect(event.message).toBe("fatal: not a git repository: ~/code/gitto");
    expect(event.exception.values[0]?.stacktrace.frames[0]?.filename).toBe("~/a.js");
    expect(event.breadcrumbs[0]).toEqual({ message: "opened ~/code/gitto", data: { count: 2 } });
  });

  it("leaves another user's home folder whose name starts the same", () => {
    expect(scrubber("/Users/mischa")("/Users/mischael/code")).toBe("/Users/mischael/code");
  });

  it("shortens a Windows home folder written with either separator", () => {
    const scrub = scrubber("C:\\Users\\mischa");

    expect(scrub({ a: "C:\\Users\\mischa\\code\\gitto", b: "C:/Users/mischa/code/gitto" })).toEqual(
      { a: "~\\code\\gitto", b: "~/code/gitto" },
    );
  });

  it("redacts the user and password in URLs", () => {
    const scrub = scrubber("/home/mischa");

    expect(
      scrub([
        "fatal: unable to access 'https://mischa:ghp_secret@github.com/msvdaamen/gitto.git/'",
        "remote: https://ghp_secret@github.com/msvdaamen/gitto.git",
        "ssh://git@github.com/msvdaamen/gitto.git",
        "git@github.com:msvdaamen/gitto.git",
      ]),
    ).toEqual([
      "fatal: unable to access 'https://[redacted]@github.com/msvdaamen/gitto.git/'",
      "remote: https://[redacted]@github.com/msvdaamen/gitto.git",
      "ssh://[redacted]@github.com/msvdaamen/gitto.git",
      "git@github.com:msvdaamen/gitto.git",
    ]);
  });

  it("leaves everything else as it is", () => {
    const scrub = scrubber("/home/mischa");
    const event = { level: "error", timestamp: 1, tags: { "rpc.path": "/git/status" }, ok: true };

    expect(scrub(structuredClone(event))).toEqual(event);
  });

  it("copes with a report that refers to itself", () => {
    const scrub = scrubber("/home/mischa");
    const event: Record<string, unknown> = { message: "/home/mischa/x" };
    event.self = event;

    expect(scrub(event)).toMatchObject({ message: "~/x", self: event });
  });
});
