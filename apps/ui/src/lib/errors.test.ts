import { describe, expect, it } from "vitest";

import { errorMessage } from "./errors";

describe("errorMessage", () => {
  it("takes an error's message, or anything else thrown as text", () => {
    expect(errorMessage(new Error("Nope."))).toBe("Nope.");
    expect(errorMessage("nope")).toBe("nope");
    expect(errorMessage(42)).toBe("42");
  });
});
