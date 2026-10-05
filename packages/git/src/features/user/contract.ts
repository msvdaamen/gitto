import { oc } from "@orpc/contract";
import * as z from "zod";

export const UserContract = {
  /**
   * The name the user commits with, from git's global config (`user.name`); `null` if it isn't
   * set, or git couldn't be asked.
   */
  name: oc.output(z.string().nullable()),
};
