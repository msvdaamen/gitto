import { z } from "zod";

/** The git Gitto runs, and whether Gitto works with it. */
export const GitInstallSchema = z.object({
  /** As git reports it, e.g. `2.39.3`; `null` if git couldn't be run (it isn't installed, say). */
  version: z.string().nullable(),
  /** The oldest version Gitto works with, e.g. `2.40`. */
  required: z.string(),
  /** Whether `version` is `required` or newer. */
  supported: z.boolean(),
});

export type GitInstall = z.infer<typeof GitInstallSchema>;
