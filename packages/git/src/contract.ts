import { CommitContract } from "./features/commit/contract";
import { DiffContract } from "./features/diff/contract";
import { HistoryContract } from "./features/history/contract";
import { RefsContract } from "./features/refs/contract";
import { StagingContract } from "./features/staging/contract";
import { StatusContract } from "./features/status/contract";
import { WatchContract } from "./features/watch/contract";

/** Every git feature's contract; the renderer calls these as `rpc.git.<feature>.<procedure>`. */
export const GitContract = {
  status: StatusContract,
  history: HistoryContract,
  diff: DiffContract,
  refs: RefsContract,
  staging: StagingContract,
  commit: CommitContract,
  watch: WatchContract,
};
