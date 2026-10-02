import { describe, expect, it } from "vitest";

import { editMessage, joinMessage, splitMessage } from "./message";

describe("commit messages", () => {
  it("splits off the first line as the summary", () => {
    expect(splitMessage("Subject\n\nBody\n#123 kept")).toEqual({
      summary: "Subject",
      description: "Body\n#123 kept",
    });
    expect(splitMessage("Subject")).toEqual({ summary: "Subject", description: "" });
  });

  it("puts a subject over several lines on one, as git shows it", () => {
    expect(splitMessage("Fix parser\nfor nested lists\n\nBody")).toEqual({
      summary: "Fix parser for nested lists",
      description: "Body",
    });
  });

  it("only counts a line of ASCII whitespace as blank, as git does", () => {
    expect(splitMessage("Subject\n\u00a0\nBody")).toEqual({
      summary: "Subject \u00a0 Body",
      description: "",
    });
  });

  it("joins the summary and the description with a blank line", () => {
    expect(joinMessage({ summary: " Subject ", description: "\nBody\n" })).toBe("Subject\n\nBody");
    expect(joinMessage({ summary: "Subject", description: "  " })).toBe("Subject");
  });

  it("round-trips a message, indented body and all", () => {
    const message = "Subject\n\n    code();\n\n## Notes";
    expect(joinMessage(splitMessage(message))).toBe(message);
  });

  it("keeps what wasn't edited exactly as written", () => {
    const original = "Fix parser\nfor nested lists  \n\n\nBody  \n";
    const split = splitMessage(original);
    expect(editMessage(original, split)).toBe(original);
    expect(editMessage(original, { ...split, description: "New body" })).toBe(
      "Fix parser\nfor nested lists  \n\n\nNew body",
    );
    expect(editMessage(original, { ...split, summary: "Fix the parser" })).toBe(
      "Fix the parser\n\n\nBody  \n",
    );
    expect(editMessage(original, { ...split, summary: `${split.summary} ` })).toBe(original);
    expect(editMessage("Subject\n", { summary: "Subject", description: "Added" })).toBe(
      "Subject\n\nAdded",
    );
  });
});
