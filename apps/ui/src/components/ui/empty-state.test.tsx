import { render, screen } from "@solidjs/testing-library";
import Inbox from "lucide-solid/icons/inbox";
import { describe, expect, it } from "vitest";

import { EmptyState } from "./empty-state";

describe("EmptyState", () => {
  it("shows the title and details", () => {
    render(() => (
      <EmptyState icon={Inbox} title="No changes">
        Your working tree is clean.
      </EmptyState>
    ));

    expect(screen.getByText("No changes")).toBeTruthy();
    expect(screen.getByText("Your working tree is clean.")).toBeTruthy();
  });

  it("highlights errors", () => {
    render(() => (
      <EmptyState icon={Inbox} title="Couldn't load changes" tone="error">
        fatal: not a git repository
      </EmptyState>
    ));

    expect(screen.getByText("fatal: not a git repository").classList).toContain("text-coral");
  });
});
