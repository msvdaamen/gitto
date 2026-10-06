import { oc } from "@orpc/contract";

import { GitInstallSchema } from "../../schema";

export const VersionContract = {
  /**
   * The installed git, and whether Gitto works with it. Until it does, every procedure on a
   * repository fails saying so. Checked again on every call until it does, e.g. after the user
   * updated git.
   */
  check: oc.output(GitInstallSchema),
};
