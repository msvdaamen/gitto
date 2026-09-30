import { oc } from "@orpc/contract";
import * as z from "zod";

export const systemContract = {
  hello: oc.input(z.object({ name: z.string() })).output(z.object({ message: z.string() })),
  /** Opens the native folder picker; resolves to `null` when the user cancels. */
  selectFolder: oc.output(z.string().nullable()),
};
