import { z } from "zod";

export const RefKindSchema = z.enum(["local", "remote", "tag"]);

export type RefKind = z.infer<typeof RefKindSchema>;

export const RefSchema = z.object({
  /** Short name, e.g. `main`, `origin/main`, `v1.0.0`. */
  name: z.string(),
  /** Full ref name, e.g. `refs/heads/main`. */
  fullName: z.string(),
  kind: RefKindSchema,
  sha: z.string(),
  current: z.boolean(),
  upstream: z.string().nullable(),
  ahead: z.number(),
  behind: z.number(),
});

export type Ref = z.infer<typeof RefSchema>;

/** The refs, and a version of them. */
export const RefListSchema = z.object({
  refs: z.array(RefSchema),
  /** Changes whenever any ref does, so a caller can ask to skip refs it already has. */
  version: z.string(),
});

export type RefList = z.infer<typeof RefListSchema>;
