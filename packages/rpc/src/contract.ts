import { GitContract } from "@gitto/git/contract";
import { RepositoryContract } from "@gitto/repository/contract";
import { SystemContract } from "@gitto/system/contract";

/** What the Electron main process serves: native OS capabilities, which only it has. */
export const mainContract = {
  system: SystemContract,
};

/**
 * What the backend process serves: everything that reads the disk or runs git. A process of its
 * own, so none of that work can keep the main process, which routes the window's input, busy.
 */
export const backendContract = {
  repository: RepositoryContract,
  git: GitContract,
};

/** Every domain's contract, merged into the single API the renderer talks to. */
export const contract = { ...mainContract, ...backendContract };
