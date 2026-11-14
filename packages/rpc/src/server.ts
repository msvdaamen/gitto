import { gitRouter } from "@gitto/git/server";
import { repositoryRouter } from "@gitto/repository/server";
import { systemRouter } from "@gitto/system/server";
import { implement, onError, ORPCError } from "@orpc/server";
import { RPCHandler } from "@orpc/server/message-port";

import type { AppContext } from "./container";
import { contract } from "./contract";

export { createContainer } from "./container";

// GITTO_TRACE=1 also logs each call's duration, next to the git commands it ran.
const tracing = !!process.env.GITTO_TRACE;

export const router = implement(contract).$context<AppContext>().router({
  system: systemRouter,
  repository: repositoryRouter,
  git: gitRouter,
});

/**
 * Serves the router over MessagePorts; call `upgrade(port, { context })` for each renderer
 * connection. `reportError` gets each error that's a bug, with the path of the call it failed.
 */
export function createRpcHandler({
  reportError,
}: { reportError?: (error: unknown, path: string) => void } = {}) {
  return new RPCHandler(router, {
    // Errors of a procedure that emits events (a clone's progress, a watch) can come once it's
    // returned its event iterator, as it's read: past the interceptors below, which see only how
    // the call went up to then.
    clientInterceptors: [
      async ({ next, path, signal }) => {
        const output = await next();
        if (!isAsyncIterator(output)) return output;
        return reportingErrors(output, (error) => {
          if (signal?.aborted) return;
          console.error("[rpc]", error);
          if (isBug(error)) reportError?.(error, `/${path.join("/")}`);
        });
      },
    ],
    interceptors: [
      async ({ next, request }) => {
        if (!tracing) return next();
        const start = performance.now();
        try {
          return await next();
        } finally {
          const ms = (performance.now() - start).toFixed(0);
          console.log(`[rpc] ${ms.padStart(5)}ms ${request.url.pathname}`);
        }
      },
      onError((error, { request }) => {
        // Cancelled requests, like queries dropped when switching repositories, aren't failures.
        if (request.signal?.aborted) return;
        console.error("[rpc]", error);
        if (isBug(error)) reportError?.(error, request.url.pathname);
      }),
    ],
  });
}

function isAsyncIterator(value: unknown): value is AsyncIterator<unknown, unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Partial<AsyncIterator<unknown>>).next === "function" &&
    Symbol.asyncIterator in value
  );
}

/** `iterator`'s events, and what it returns, with each error it throws told to `report` too. */
async function* reportingErrors<T, R>(
  iterator: AsyncIterator<T, R>,
  report: (error: unknown) => void,
): AsyncGenerator<T, R> {
  try {
    // Stopping this (as the renderer does when it's no longer read) stops `iterator` too.
    return yield* { [Symbol.asyncIterator]: () => iterator };
  } catch (error) {
    report(error);
    throw error;
  }
}

/**
 * Whether an error is a bug, or git failing in a way Gitto doesn't expect, rather than one the user
 * is told about as it's how things are, like a conflict (see `toApiError` in @gitto/git). Input
 * that doesn't fit the contract is the renderer's bug.
 */
function isBug(error: unknown) {
  if (!(error instanceof ORPCError)) return true;
  return error.status >= 500 || error.code === "BAD_REQUEST";
}
