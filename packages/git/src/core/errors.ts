import { ORPCError } from "@orpc/server";

import { GitError } from "./runner";

/** Turns a failed git command into an error the renderer can show. */
export function toApiError(error: unknown, cwd: string): unknown {
  if (!(error instanceof GitError)) return error;
  if (/not a git repository/i.test(error.stderr)) {
    return new ORPCError("NOT_FOUND", { message: `${cwd} is no longer a git repository.` });
  }
  if (/index\.lock/.test(error.stderr)) {
    return new ORPCError("CONFLICT", {
      message: "Another git process is running in this repository.",
    });
  }
  return new ORPCError("INTERNAL_SERVER_ERROR", { message: error.message, cause: error });
}
