import { describe, expect, it } from "vitest";

import { ConflictCounter, countConflicts, isBinary } from "./markers";

describe("countConflicts", () => {
  it("counts the conflicts git leaves, in each conflict style", () => {
    const merge = "a\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> side\nb\n";
    const diff3 = "<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> side\n";
    const zdiff3 = "<<<<<<< HEAD\nours\n||||||| base\n=======\n>>>>>>> side\n";
    expect(countConflicts(merge)).toBe(1);
    expect(countConflicts(diff3)).toBe(1);
    expect(countConflicts(zdiff3)).toBe(1);
    expect(countConflicts(merge + diff3 + zdiff3)).toBe(3);
  });

  it("reads CRLF line ends, and a file that ends without one", () => {
    expect(countConflicts("<<<<<<< HEAD\r\na\r\n=======\r\nb\r\n>>>>>>> side\r\n")).toBe(1);
    expect(countConflicts("<<<<<<< HEAD\na\n=======\nb\n>>>>>>> side")).toBe(1);
  });

  it("counts conflicts inside conflicts once each", () => {
    const nested = [
      "<<<<<<< HEAD",
      "<<<<<<<<< Temporary merge branch 1",
      "a",
      "=========",
      "b",
      ">>>>>>>>> Temporary merge branch 2",
      "=======",
      "c",
      ">>>>>>> side",
      "",
    ].join("\n");
    expect(countConflicts(nested)).toBe(2);
  });

  it("doesn't take what only looks like markers for a conflict", () => {
    expect(countConflicts("")).toBe(0);
    expect(countConflicts("Title\n=======\n\ntext\n")).toBe(0);
    expect(countConflicts("<<<<<<< HEAD\nunfinished\n")).toBe(0);
    expect(countConflicts("<<<<<< short\n=======\n>>>>>> short\n")).toBe(0);
    expect(countConflicts("<<<<<<<HEAD\n=======\n>>>>>>>side\n")).toBe(0);
    expect(countConflicts("<<<<<<< HEAD\nno separator\n>>>>>>> side\n")).toBe(0);
  });
});

describe("ConflictCounter", () => {
  it("counts the same however the text comes in pieces", () => {
    const text =
      "a\n<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> side\nb\n<<<<<<< x\n=======\n>>>>>>> y";
    for (let at = 0; at <= text.length; at++) {
      const counter = new ConflictCounter();
      counter.push(text.slice(0, at));
      counter.push(text.slice(at));
      expect(counter.end()).toBe(2);
    }
  });

  it("keeps only the start of a line too long to be a marker's", () => {
    const counter = new ConflictCounter();
    counter.push("<<<<<<< HEAD\nours\n");
    // A separator's run of = signs, longer than any marker line is kept: not a separator.
    counter.push("=".repeat(10_000));
    counter.push("\ntheirs\n>>>>>>> side\n");
    expect(counter.end()).toBe(0);
  });
});

describe("isBinary", () => {
  it("looks for a NUL in the first 8000 bytes, as git does", () => {
    expect(isBinary(Buffer.from("text\n"))).toBe(false);
    expect(isBinary(Buffer.from([1, 0, 2]))).toBe(true);
    expect(isBinary(Buffer.concat([Buffer.alloc(8000, 97), Buffer.from([0])]))).toBe(false);
  });
});
