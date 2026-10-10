import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { UpdateStateSchema, VersionChangesSchema } from "./schema";

export const SystemContract = {
  /** Opens the native folder picker; resolves to `null` when the user cancels. */
  selectFolder: oc.output(z.string().nullable()),
  update: {
    /** Emits the running version and the update it's getting: at once, then whenever it changes. */
    watch: oc.output(eventIterator(UpdateStateSchema)),
    /** Restarts Gitto into the update, once it's ready. */
    install: oc.output(z.void()),
  },
  changelog: {
    /**
     * What changed for users in the versions since the last one they saw, up to this one, the
     * newest first. Empty on Gitto's first run, as there's nothing to compare with.
     */
    unseen: oc.output(z.array(VersionChangesSchema)),
    /** What changed for users in every version this one knows of, the newest first. */
    all: oc.output(z.array(VersionChangesSchema)),
    /** Remembers this version as the last one the user saw what changed in. */
    seen: oc.output(z.void()),
  },
};
