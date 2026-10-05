import { BranchContract } from "./features/branch/contract";
import { CommitContract } from "./features/commit/contract";
import { DiffContract } from "./features/diff/contract";
import { HistoryContract } from "./features/history/contract";
import { OverviewContract } from "./features/overview/contract";
import { RefsContract } from "./features/refs/contract";
import { RemoteContract } from "./features/remote/contract";
import { StagingContract } from "./features/staging/contract";
import { StashContract } from "./features/stash/contract";
import { StatusContract } from "./features/status/contract";
import { UserContract } from "./features/user/contract";
import { VersionContract } from "./features/version/contract";
import { WatchContract } from "./features/watch/contract";

/** Every git feature's contract; the renderer calls these as `rpc.git.<feature>.<procedure>`. */
export const GitContract = {
  status: StatusContract,
  history: HistoryContract,
  diff: DiffContract,
  refs: RefsContract,
  branch: BranchContract,
  staging: StagingContract,
  commit: CommitContract,
  remote: RemoteContract,
  stash: StashContract,
  overview: OverviewContract,
  watch: WatchContract,
  version: VersionContract,
  user: UserContract,
};
