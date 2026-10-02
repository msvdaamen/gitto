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

  it("joins the summary and the description with a blank line", () => {
    expect(joinMessage({ summary: " Subject ", description: "\nBody\n" })).toBe("Subject\n\nBody");
    expect(joinMessage({ summary: "Subject", description: "  " })).toBe("Subject");
  });

  it("round-trips a message", () => {
    const message = "Subject\n\nBody\n\n## Notes";
    expect(joinMessage(splitMessage(message))).toBe(message);
  });
});
