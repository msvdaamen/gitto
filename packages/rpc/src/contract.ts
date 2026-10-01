import { GitContract } from "@gitto/git/contract";
import { RepositoryContract } from "@gitto/repository/contract";
import { SystemContract } from "@gitto/system/contract";

/** Every domain's contract, merged into the single API the renderer talks to. */
export const contract = {
  system: SystemContract,
  repository: RepositoryContract,
  git: GitContract,
};
