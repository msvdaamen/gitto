import { CHANGE_TYPES } from "@gitto/release/changelog";
import { z } from "zod";

/**
 * Where a build gets its updates from: a nightly only from the next nightly, a release only from
 * the next release.
 */
export const UpdateChannelSchema = z.enum(["release", "nightly"]);

/** The running Gitto, and the update it's getting. */
export const UpdateStateSchema = z.object({
  /** The running version, e.g. `1.2.3` or `1.2.4-nightly29853462`. */
  version: z.string(),
  channel: UpdateChannelSchema,
  /** A newer version, while it's downloaded, then once it's installed on restart. */
  update: z.object({ version: z.string(), ready: z.boolean() }).nullable(),
});

export type UpdateChannel = z.infer<typeof UpdateChannelSchema>;
export type UpdateState = z.infer<typeof UpdateStateSchema>;

/** A change users see, from its commit's Conventional Commits title (see @gitto/release). */
export const ChangeSchema = z.object({
  type: z.enum(CHANGE_TYPES),
  scope: z.string().optional(),
  breaking: z.boolean(),
  /** As it's titled, without the type: `add worktrees`. */
  description: z.string(),
  /** What users are told about it beyond its title. */
  detail: z.string().optional(),
  pr: z.number().optional(),
  commit: z.string(),
});

/** A version, and what changed in it since the one before it on its channel. */
export const VersionChangesSchema = z.object({
  version: z.string(),
  /** When it was built, as YYYY-MM-DD. */
  date: z.string(),
  changes: z.array(ChangeSchema),
});

export type Change = z.infer<typeof ChangeSchema>;
export type VersionChanges = z.infer<typeof VersionChangesSchema>;
