import * as z from "zod";

/** Input shared by every procedure that works on a repository; see `withRepo`. */
export const RepositoryInput = z.object({ repositoryId: z.uuidv7() });

/** A full or abbreviated commit SHA. */
export const Sha = z.string().regex(/^[0-9a-f]{4,64}$/);

/** A full commit SHA: SHA-1, or SHA-256. */
export const FullSha = z.string().regex(/^[0-9a-f]{40}([0-9a-f]{24})?$/);
