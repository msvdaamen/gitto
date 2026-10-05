import { z } from "zod";

/**
 * Where a build gets its updates from: a nightly only from the next nightly, a release only from
 * the next release.
 */
export const UpdateChannelSchema = z.enum(["release", "nightly"]);

/** The running Gitto, and the update it's getting. */
export const UpdateStateSchema = z.object({
  /** The running version, e.g. `1.2.3` or `1.2.4-nightly.20261005134259`. */
  version: z.string(),
  channel: UpdateChannelSchema,
  /** A newer version, while it's downloaded, then once it's installed on restart. */
  update: z.object({ version: z.string(), ready: z.boolean() }).nullable(),
});

export type UpdateChannel = z.infer<typeof UpdateChannelSchema>;
export type UpdateState = z.infer<typeof UpdateStateSchema>;
