import { eventIterator, oc } from "@orpc/contract";
import * as z from "zod";

import { UpdateStateSchema } from "./schema";

export const SystemContract = {
  /** Opens the native folder picker; resolves to `null` when the user cancels. */
  selectFolder: oc.output(z.string().nullable()),
  update: {
    /** Emits the running version and the update it's getting: at once, then whenever it changes. */
    watch: oc.output(eventIterator(UpdateStateSchema)),
    /** Restarts Gitto into the update, once it's ready. */
    install: oc.output(z.void()),
  },
};
