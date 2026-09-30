import { z } from "zod";

export const RefSchema = z.object({
  /** Short name, e.g. `main`, `origin/main`, `v1.0.0`. */
  name: z.string(),
  /** Full ref name, e.g. `refs/heads/main`. */
  fullName: z.string(),
  kind: z.enum(["local", "remote", "tag"]),
  sha: z.string(),
  current: z.boolean(),
  upstream: z.string().nullable(),
  ahead: z.number(),
  behind: z.number(),
});

export type Ref = z.infer<typeof RefSchema>;
