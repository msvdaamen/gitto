import { RepositoryContract } from "@gitto/repository/contract";
import { systemContract } from "@gitto/system/contract";

/** Every domain's contract, merged into the single API the renderer talks to. */
export const contract = {
  system: systemContract,
  repository: RepositoryContract,
};
