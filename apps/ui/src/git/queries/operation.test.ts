import { renderHook } from "@solidjs/testing-library";
import type { JSX } from "@solidjs/web";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { createComponent, flush } from "solid-js";
import { describe, expect, it } from "vitest";

import { useRepositoryOperation } from "./operation";

function render<T>(hook: () => T) {
  const client = new QueryClient();
  return renderHook(hook, {
    wrapper: (props: { children: JSX.Element }) =>
      createComponent(QueryClientProvider, {
        client,
        get children() {
          return props.children;
        },
      }),
  }).result;
}

describe("a repository operation", () => {
  it("fails, rather than running on, when it throws rather than rejecting", async () => {
    const operation = render(() =>
      useRepositoryOperation(
        "continue",
        () => "repo",
        () => {
          throw new Error("The connection is closed.");
        },
      ),
    );

    operation.run();
    flush();
    await Promise.resolve();
    flush();

    expect(operation.isPending()).toBe(false);
    expect(operation.error()?.message).toBe("The connection is closed.");
  });
});
