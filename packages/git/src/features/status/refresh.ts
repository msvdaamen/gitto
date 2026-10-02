import type { Repo } from "../../core/repo";
import { trace } from "../../core/trace";
import { gitDirs, indexSignature } from "../watch/git-dirs";

/** A status that takes this long may be held up by a stale index. */
const SLOW_MS = 500;
/** How long a repository's index is left alone after refreshing it. */
const COOLDOWN_MS = 60_000;
/** The same, when refreshing found nothing stale: the status is just that slow there. */
const USELESS_COOLDOWN_MS = 30 * 60_000;

/** When each repository's index can be refreshed again, by its path. */
const notBefore = new Map<string, number>();

/**
 * Refreshes the index after a slow status, if it's been a while: resolves to whether it did. Never
 * rejects, as nobody waits for it.
 *
 * The index remembers each file's size and modification time, to tell that a file hasn't changed
 * without reading it. When files are written without changing (a build, a formatter, a restored
 * backup), those no longer match, and git has to read every one of them to find out. It then
 * remembers the new times, but not when run the way Gitto runs it (see GIT_OPTIONAL_LOCKS in the
 * runner), so every status stays that slow until some other tool refreshes the index. This does
 * what `git status` in a terminal would have: it takes the index lock while it works, which is why
 * it's only done when the status is slow, not again soon, and gives way to the user's own writes.
 */
export async function refreshStaleIndex(
  repo: Repo,
  statusMs: number,
  slowMs = SLOW_MS,
): Promise<boolean> {
  const now = performance.now();
  if (statusMs < slowMs || now < (notBefore.get(repo.path) ?? 0)) return false;
  // Set before anything's awaited, so statuses running side by side don't each refresh.
  notBefore.set(repo.path, now + COOLDOWN_MS);

  try {
    const dirs = await gitDirs(repo);
    const before = await indexSignature(dirs);
    // `-q` and `--unmerged`: files that did change, and conflicts, aren't errors.
    await repo.writeInBackground(["update-index", "-q", "--unmerged", "--refresh"]);
    // Git only writes the index when something in it was out of date.
    const stale = (await indexSignature(dirs)) !== before;
    if (!stale) notBefore.set(repo.path, now + USELESS_COOLDOWN_MS);
    const took = (performance.now() - now).toFixed(0);
    trace(
      `refreshed the index of ${repo.path} in ${took}ms after a ${statusMs.toFixed(0)}ms status: ` +
        (stale ? "it was stale" : "nothing was stale"),
    );
    return true;
  } catch (error) {
    // Another git process holds the index, a write of Gitto's came in, or the repository is gone.
    // Tried again after the next slow status, once it's been a while.
    trace(`didn't refresh the index of ${repo.path}: ${(error as Error).message}`);
    return false;
  }
}
