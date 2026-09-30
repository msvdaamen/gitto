// Adds DOM matchers like `toBeInTheDocument()` and `toHaveClass()` to `expect`.
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@solidjs/testing-library";
import { afterEach } from "vitest";

// Testing Library only unmounts rendered components by itself when Vitest's globals are on.
afterEach(cleanup);
