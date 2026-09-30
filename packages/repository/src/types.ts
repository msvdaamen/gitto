import { z } from "zod";

export const RepositorySchema = z.object({
  id: z.uuidv7(),
  name: z.string(),
  path: z.string(),
});

export type Repository = z.infer<typeof RepositorySchema>;
