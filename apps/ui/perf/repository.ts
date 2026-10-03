// A made-up repository for the renderer's perf suites, the same on every run: a history like a
// busy project's, with feature branches that fork off main and are merged back, and the refs that
// point into it.
import type { Commit, CommitRef, Ref } from "@gitto/git/types";

export interface FakeRepository {
  commits: Commit[];
  refs: Ref[];
}

const AUTHORS = [
  "Ada Lovelace",
  "Grace Hopper",
  "Alan Turing",
  "Katherine Johnson",
  "Edsger Dijkstra",
  "Barbara Liskov",
  "Donald Knuth",
  "Margaret Hamilton",
  "Ken Thompson",
  "Frances Allen",
  "Dennis Ritchie",
  "Radia Perlman",
];

const VERBS = ["Fix", "Add", "Remove", "Update", "Move", "Rename", "Speed up", "Simplify", "Show"];
const THINGS = [
  "the history graph",
  "the branch list in the sidebar",
  "a crash when the repository has no commits",
  "the commit details",
  "the changed files' line counts",
  "keyboard navigation in the history",
  "the status after a rebase",
  "tags on remote branches",
  "the diff of renamed files",
  "the toolbar's pull button",
];
const REASONS = [
  "",
  "",
  " when the window is narrow",
  " for repositories with thousands of branches",
  " so it no longer blocks the UI while git runs",
  " after a fetch that brings in new tags",
];

/** A deterministic pseudo-random number generator (mulberry32), so every run sees the same data. */
export function random(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * A history of `count` commits, newest first like the log, with up to `lanes` feature branches
 * open next to main at a time. Each is merged back once it has a few commits; the ones still open
 * at the end get a local and a remote branch, and every 50th commit on main is tagged.
 */
export function fakeRepository(count: number, lanes = 6): FakeRepository {
  const next = random(count);
  const pick = <T>(items: T[]) => items[Math.floor(next() * items.length)]!;
  const sha = () =>
    Array.from({ length: 5 }, () =>
      Math.floor(next() * 2 ** 32)
        .toString(16)
        .padStart(8, "0"),
    ).join("");

  const commits: Commit[] = [];
  let time = Date.UTC(2024, 0, 1);
  const commit = (parents: string[], subject: string): Commit => {
    time += Math.floor(next() * 3_600_000);
    const author = pick(AUTHORS);
    const created: Commit = {
      sha: sha(),
      parents,
      authorName: author,
      authorEmail: `${author.toLowerCase().replace(" ", ".")}@example.com`,
      authoredAt: time,
      committedAt: time,
      refs: [],
      subject,
      body: next() < 0.3 ? "A longer explanation of why.\n\nOver a few lines." : "",
    };
    commits.push(created);
    return created;
  };
  const subject = () => `${pick(VERBS)} ${pick(THINGS)}${pick(REASONS)}`;

  let main = commit([], "Initial commit");
  const branches: { name: string; tip: Commit; length: number }[] = [];
  let mainCommits = 0;
  while (commits.length < count) {
    const roll = next();
    if (branches.length < lanes && roll < 0.1) {
      const tip = commit([main.sha], subject());
      branches.push({ name: `feature/${commits.length}`, tip, length: 1 });
    } else if (branches.length > 0 && roll < 0.75) {
      const branch = pick(branches);
      if (branch.length >= 3 && next() < 0.25) {
        main = commit([main.sha, branch.tip.sha], `Merge branch '${branch.name}'`);
        branches.splice(branches.indexOf(branch), 1);
      } else {
        branch.tip = commit([branch.tip.sha], subject());
        branch.length++;
      }
    } else {
      main = commit([main.sha], subject());
      if (++mainCommits % 50 === 0) main.refs.push({ kind: "tag", name: `v1.${mainCommits / 50}` });
    }
  }
  main.refs.push({ kind: "local", name: "main", current: true });
  main.refs.push({ kind: "remote", name: "origin/main" });
  for (const branch of branches) {
    branch.tip.refs.push({ kind: "local", name: branch.name });
    branch.tip.refs.push({ kind: "remote", name: `origin/${branch.name}` });
  }

  commits.reverse();
  return { commits, refs: commits.flatMap((each) => each.refs.map((ref) => toRef(each, ref))) };
}

function toRef(commit: Commit, ref: CommitRef): Ref {
  const kind = ref.kind === "head" ? "local" : ref.kind;
  const prefix = { local: "refs/heads/", remote: "refs/remotes/", tag: "refs/tags/" }[kind];
  return {
    name: ref.name,
    fullName: `${prefix}${ref.name}`,
    kind,
    sha: commit.sha,
    current: ref.current ?? false,
    upstream: kind === "local" ? `origin/${ref.name}` : null,
    ahead: 0,
    behind: 0,
  };
}
