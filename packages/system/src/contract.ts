import { oc } from "@orpc/contract";
import * as z from "zod";

export const SystemContract = {
  /** Opens the native folder picker; resolves to `null` when the user cancels. */
  selectFolder: oc.output(z.string().nullable()),
};
