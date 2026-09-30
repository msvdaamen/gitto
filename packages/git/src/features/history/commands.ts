import type { Repo } from "../../core/repo";
import { LOG_FORMAT, parseLog } from "./parse";
import type { Commit } from "./schema";

export async function getLog(
  repo: Repo,
  page: { limit: number; skip: number },
  signal?: AbortSignal,
): Promise<Commit[]> {
  // Only refs a user would recognise; `--all` also picks up tool namespaces (e.g. refs/t3/*).
  // HEAD is included for detached checkouts, unless the branch has no commits yet.
  const revisions = [
    "--branches",
    "--remotes",
    "--tags",
    ...((await repo.hasHead()) ? ["HEAD"] : []),
  ];
  const output = await repo.read(
    [
      "log",
      "-z",
      LOG_FORMAT,
      "--topo-order",
      "--decorate=short",
      `--max-count=${page.limit}`,
      `--skip=${page.skip}`,
      ...revisions,
      "--",
    ],
    { signal },
  );
  return parseLog(output);
}
