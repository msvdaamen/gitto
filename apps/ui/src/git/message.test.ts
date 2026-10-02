import { describe, expect, it } from "vitest";

import { joinMessage, splitMessage } from "./message";

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

  it("joins the summary and the description with a blank line", () => {
    expect(joinMessage({ summary: " Subject ", description: "\nBody\n" })).toBe("Subject\n\nBody");
    expect(joinMessage({ summary: "Subject", description: "  " })).toBe("Subject");
  });

  it("round-trips a message, indented body and all", () => {
    const message = "Subject\n\n    code();\n\n## Notes";
    expect(joinMessage(splitMessage(message))).toBe(message);
  });
});
