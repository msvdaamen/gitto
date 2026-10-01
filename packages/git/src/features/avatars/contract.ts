import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput, Sha } from "../../input";

export const AvatarsContract = {
  /**
   * The URL of an author's profile picture: their GitHub or GitLab avatar when the repository is
   * hosted there, their Gravatar otherwise. It may not load, when they have none.
   */
  find: oc
    .input(RepositoryInput.extend({ email: z.string(), sha: Sha }))
    .output(z.string().nullable()),
};
