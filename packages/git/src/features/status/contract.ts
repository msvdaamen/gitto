import { oc } from "@orpc/contract";

import { RepositoryInput } from "../../input";
import { StatusSchema } from "./schema";

export const StatusContract = {
  get: oc.input(RepositoryInput).output(StatusSchema),
};
