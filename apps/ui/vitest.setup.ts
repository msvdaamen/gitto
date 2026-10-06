// Adds DOM matchers like `toBeInTheDocument()` and `toHaveClass()` to `expect`.
import "@testing-library/jest-dom/vitest";
import { TransformStream } from "node:stream/web";

import { cleanup } from "@solidjs/testing-library";
import { afterEach } from "vitest";

// The vm pool's context (see vitest.config.ts) has no web streams, which the diff library's worker
// protocol needs when it's imported.
if (typeof globalThis.TransformStream === "undefined") {
  Object.assign(globalThis, { TransformStream });
}

// Testing Library only unmounts rendered components by itself when Vitest's globals are on.
afterEach(cleanup);
