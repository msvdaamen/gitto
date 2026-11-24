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

// What UI SDK's popups need of a browser that jsdom doesn't have: they render in the top layer, as in
// the app, rather than at the end of the body. Assigned rather than only filled in, as jsdom has some
// without the parts that matter.
HTMLElement.prototype.showPopover = function () {};
HTMLElement.prototype.hidePopover = function () {};
Element.prototype.getAnimations = () => [];
Element.prototype.scrollIntoView = function () {};
Element.prototype.checkVisibility = () => true;
Element.prototype.getClientRects = () => [] as unknown as DOMRectList;
document.elementFromPoint = () => null;
document.elementsFromPoint = () => [];
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
// The vm pool's context has no performance timeline, which UI SDK reads as popups open.
performance.getEntriesByType ??= () => [];
document.head.append(
  Object.assign(document.createElement("style"), {
    textContent: "[popover] { display: block !important }",
  }),
);

// Testing Library only unmounts rendered components by itself when Vitest's globals are on.
afterEach(cleanup);
