import { execFileSync } from "node:child_process";

import { describe, expect, it } from "vitest";

import { createRepo, git, repos } from "../../test/fixtures";
import { CommitIndex } from "./commit-index";

/** A commit made at `seconds`, with `parents`, that nothing points at yet. */
function commit(path: string, seconds: number, message: string, ...parents: string[]): string {
  const tree = git(path, "hash-object", "-t", "tree", "/dev/null");
  const date = `@${seconds} +0000`;
  return execFileSync(
    "git",
    ["commit-tree", tree, ...parents.flatMap((p) => ["-p", p]), "-m", message],
    {
      cwd: path,
      encoding: "utf8",
      env: { ...process.env, GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date },
    },
  ).trim();
}

describe("the commit index", () => {
  it("has the commits its tips reach, however many", async () => {
    const path = createRepo("index-big");
    // 3,000 commits on main, with a merge of a side branch every hundred, at once.
    const lines: string[] = [];
    for (let i = 1; i <= 3000; i++) {
      const side = i % 100 === 0;
      lines.push(
        `commit refs/heads/${side ? "side" : "main"}`,
        `mark :${i}`,
        `committer T <t@t> ${1000 + i} +0000`,
        "data 0",
        ...(i > 1 ? [`from :${side ? i - 1 : i - (i % 100 === 1 && i > 100 ? 2 : 1)}`] : []),
        ...(i % 100 === 1 && i > 100 ? [`merge :${i - 1}`] : []),
        "",
      );
    }
    execFileSync("git", ["fast-import", "--quiet"], { cwd: path, input: lines.join("\n") });
    const repo = await repos.open("index-big");
    const tips = new Set([git(path, "rev-parse", "main"), git(path, "rev-parse", "side")]);

    const index = await CommitIndex.load(repo, tips);
    const all = git(path, "rev-list", "main", "side").split("\n");
    expect(all).toHaveLength(3000);
    expect(index.size).toBe(all.length);
    expect(all.every((sha) => index.has(sha))).toBe(true);
    expect(index.has("0".repeat(40))).toBe(false);
  });

  it("tells what moving the tips added and lost", async () => {
    const path = createRepo("index-sync");
    const repo = await repos.open("index-sync");
    const first = commit(path, 1000, "first");
    const second = commit(path, 1001, "second", first);
    const index = await CommitIndex.load(repo, new Set([second]));

    const third = commit(path, 1002, "third", second);
    expect(await index.sync(repo, new Set([third]))).toEqual({
      added: new Set([third]),
      lost: false,
    });

    // Reset back: the commit isn't reachable anymore, then is again.
    expect(await index.sync(repo, new Set([first]))).toEqual({ added: new Set(), lost: true });
    expect(index.has(second)).toBe(false);
    expect(index.has(first)).toBe(true);
    expect(await index.sync(repo, new Set([third]))).toEqual({
      added: new Set([second, third]),
      lost: false,
    });

    // A branch that's gone, but whose commit another reaches: nothing lost.
    const other = commit(path, 1003, "other", first);
    await index.sync(repo, new Set([third, other, second]));
    expect(await index.sync(repo, new Set([third, other]))).toEqual({
      added: new Set(),
      lost: false,
    });
  });

  it("doesn't ask git about tips that were reachable already", async () => {
    const path = createRepo("index-known");
    const repo = await repos.open("index-known");
    const first = commit(path, 1000, "first");
    const second = commit(path, 1001, "second", first);
    const index = await CommitIndex.load(repo, new Set([second]));
    const reads: string[][] = [];
    const recording = { ...repo, read: (args: string[]) => (reads.push(args), repo.read(args)) };

    // A branch made at the first commit, then the second one's branch moved back to it.
    expect(await index.sync(recording, new Set([second, first]))).toEqual({
      added: new Set(),
      lost: false,
    });
    expect(await index.sync(recording, new Set([first]))).toEqual({ added: new Set(), lost: true });
    expect(reads).toEqual([]);
  });

  it("leaves out commits git lists as new that weren't, when the clock was skewed", async () => {
    const path = createRepo("index-skew");
    const repo = await repos.open("index-skew");
    // A commit, then eight on top of it dated well before it: walking by date from `old`, git
    // reaches `skewed` only after giving up.
    const root = commit(path, 1000, "root");
    const skewed = commit(path, 10_000, "skewed", root);
    let old = skewed;
    for (let i = 1; i <= 8; i++) old = commit(path, 100 + i, `behind ${i}`, old);
    const fresh = commit(path, 20_000, "new", skewed);
    // Git's own answer: wrong.
    expect(git(path, "rev-list", fresh, `^${old}`).split("\n")).toContain(skewed);

    const index = await CommitIndex.load(repo, new Set([old]));
    expect(await index.sync(repo, new Set([old, fresh]))).toEqual({
      added: new Set([fresh]),
      lost: false,
    });
  });

  it("is brought up to date once at a time", async () => {
    const path = createRepo("index-race");
    const repo = await repos.open("index-race");
    const first = commit(path, 1000, "first");
    const index = await CommitIndex.load(repo, new Set([first]));
    const [a, b] = [commit(path, 1001, "a", first), commit(path, 1002, "b", first)];

    const [one, two] = await Promise.all([
      index.sync(repo, new Set([a])),
      index.sync(repo, new Set([b])),
    ]);
    // The first one done wins; the other changes nothing.
    expect([one, two].filter(Boolean)).toHaveLength(1);
    expect(index.has(a) !== index.has(b)).toBe(true);
  });
});
