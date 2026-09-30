import { oc } from "@orpc/contract";
import * as z from "zod";

import { RepositoryInput } from "../../input";

const PathsInput = RepositoryInput.extend({ paths: z.array(z.string().min(1)).min(1) });

export const StagingContract = {
  stage: oc.input(PathsInput),
  unstage: oc.input(PathsInput),
};
