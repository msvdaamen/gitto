import * as z from "zod";

/** Input shared by every procedure that works on a repository; see `withRepo`. */
export const RepositoryInput = z.object({ repositoryId: z.uuidv7() });
