import * as z from "zod";

/** Input shared by every procedure that works on a repository; see `withRepo`. */
export const RepositoryInput = z.object({ repositoryId: z.uuidv7() });

/** A full or abbreviated commit SHA. */
export const Sha = z.string().regex(/^[0-9a-f]{4,64}$/);
