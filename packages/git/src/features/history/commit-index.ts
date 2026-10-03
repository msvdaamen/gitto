// The commits the history reaches, kept in the main process, so that what a change to the refs
// added to the history and took from it is known exactly, without git walking it all again.
import type { Repo } from "../../core/repo";

/** Each hex digit's value, by character code. */
const HEX = new Uint8Array(128);
for (let i = 0; i < 16; i++) HEX["0123456789abcdef".charCodeAt(i)] = i;

/** The byte at `byte` of the hex SHA `sha`. */
function hexByte(sha: string, byte: number): number {
  return (HEX[sha.charCodeAt(2 * byte)]! << 4) | HEX[sha.charCodeAt(2 * byte + 1)]!;
}

/** `array`'s values in a new array of `length`. */
function grown<T extends Uint8Array | Int32Array>(array: T, length: number): T {
  const bigger = new (array.constructor as new (length: number) => T)(length);
  bigger.set(array);
  return bigger;
}

/** What changed when the index was brought up to date with new tips. */
export interface IndexChanges {
  /** The commits reachable from the new tips that weren't from the old ones. */
  added: Set<string>;
  /** Whether any commit reachable from the old tips isn't from the new ones. */
  lost: boolean;
}

/**
 * The commits reachable from a set of tips (the commits the history starts from), with their
 * parents. Git can tell what's reachable from some commits but not from others (`rev-list new
 * ^old`), but stops walking by commit date: with a skewed clock (a commit dated before its parent),
 * it lists commits that were reachable already. Those are told apart here.
 *
 * Compact, as a repository can have millions of commits: each commit's SHA, first parent and
 * whether it's reachable are in arrays, found by a hash table of their SHAs (vscode's 190,000 take
 * about 10MB).
 */
export class CommitIndex {
  /** By the first bytes of a SHA, its commit's position plus one (0 for none); half empty. */
  private slots = new Int32Array(1 << 12);
  private shas: Uint8Array;
  /** -1 for none. */
  private firstParents = new Int32Array(1 << 11);
  private otherParents = new Map<number, number[]>();
  /** 1 for commits reachable from `tips`; the others are kept, in case they come back. */
  private reachable = new Uint8Array(1 << 11);
  private count = 0;

  private constructor(
    /** Bytes in a SHA: 20, or 32 in a SHA-256 repository. */
    private readonly width: number,
    /** The commits it has those reachable from. */
    public tips: ReadonlySet<string>,
  ) {
    this.shas = new Uint8Array(this.firstParents.length * width);
  }

  /**
   * The commits reachable from `tips`, read from git as it lists them, so a big history doesn't
   * hold up the main process.
   */
  static async load(repo: Repo, tips: ReadonlySet<string>, signal?: AbortSignal) {
    const [first] = tips;
    const index = new CommitIndex(first ? first.length / 2 : 20, tips);
    if (!first) return index;
    let rest = "";
    await repo.read(["rev-list", "--parents", "--stdin"], {
      signal,
      stdin: `${[...tips].join("\n")}\n`,
      onStdout: (chunk) => {
        const text = rest + chunk.toString("latin1");
        const end = text.lastIndexOf("\n");
        rest = text.slice(end + 1);
        for (const line of text.slice(0, end).split("\n")) if (line) index.define(line);
      },
    });
    if (rest) index.define(rest);
    return index;
  }

  /** How many commits it has, reachable or not. */
  get size(): number {
    return this.count;
  }

  /** Whether `sha` is reachable from the tips. */
  has(sha: string): boolean {
    const id = this.find(sha);
    return id !== -1 && this.reachable[id] === 1;
  }

  /**
   * Brings it up to date with `tips`: asks git for what's reachable from them but wasn't from the
   * old ones, leaves out what it lists that was, and works out whether anything isn't reachable
   * anymore. `undefined`, having changed nothing, if it was brought up to date meanwhile.
   */
  async sync(
    repo: Repo,
    tips: ReadonlySet<string>,
    signal?: AbortSignal,
  ): Promise<IndexChanges | undefined> {
    const from = this.tips;
    const fresh = [...tips].filter((tip) => !from.has(tip));
    const listed = fresh.length
      ? await repo.read(["rev-list", "--parents", "--stdin"], {
          signal,
          stdin: `${[...fresh, ...[...from].map((tip) => `^${tip}`)].join("\n")}\n`,
        })
      : "";
    if (this.tips !== from) return undefined;

    const added = new Set<string>();
    for (const line of listed.split("\n")) {
      const sha = line.split(" ", 1)[0]!;
      // Git lists commits reachable already when the clock was skewed.
      if (!sha || this.has(sha)) continue;
      this.define(line);
      added.add(sha);
    }
    this.tips = tips;

    // A tip that's gone can still be reachable, e.g. a branch's old commit, now the parent of a new
    // one. Otherwise, what's reachable is worked out again.
    const kept = new Set(tips);
    for (const sha of added) {
      for (const parent of this.parentsOf(this.find(sha))) kept.add(this.shaOf(parent));
    }
    const gone = [...from].filter((tip) => !kept.has(tip));
    return { added, lost: gone.length > 0 && this.markReachable() };
  }

  /** Marks what's reachable from the tips again; whether anything that was, isn't anymore. */
  private markReachable(): boolean {
    const marked = new Uint8Array(this.reachable.length);
    const stack: number[] = [];
    const visit = (id: number) => {
      if (id === -1 || marked[id]) return;
      marked[id] = 1;
      stack.push(id);
    };
    for (const tip of this.tips) visit(this.find(tip));
    while (stack.length > 0) {
      const id = stack.pop()!;
      for (const parent of this.parentsOf(id)) visit(parent);
    }
    let lost = false;
    for (let id = 0; id < this.count && !lost; id++) {
      lost = this.reachable[id] === 1 && marked[id] === 0;
    }
    this.reachable = marked;
    return lost;
  }

  /** Adds the commit `rev-list --parents` describes with `line`, as reachable. */
  private define(line: string): void {
    const [sha, ...parents] = line.split(" ");
    const id = this.intern(sha!);
    this.reachable[id] = 1;
    const parentIds = parents.map((parent) => this.intern(parent));
    this.firstParents[id] = parentIds[0] ?? -1;
    if (parentIds.length > 1) this.otherParents.set(id, parentIds.slice(1));
    else this.otherParents.delete(id);
  }

  private *parentsOf(id: number): Iterable<number> {
    if (id === -1 || this.firstParents[id] === -1) return;
    yield this.firstParents[id]!;
    yield* this.otherParents.get(id) ?? [];
  }

  private shaOf(id: number): string {
    let sha = "";
    for (let byte = 0; byte < this.width; byte++) {
      sha += this.shas[id * this.width + byte]!.toString(16).padStart(2, "0");
    }
    return sha;
  }

  /** `sha`'s position; -1 if it hasn't got one. */
  private find(sha: string): number {
    const mask = this.slots.length - 1;
    for (let slot = this.hashOf(sha) & mask; ; slot = (slot + 1) & mask) {
      const id = this.slots[slot]! - 1;
      if (id === -1 || this.matches(id, sha)) return id;
    }
  }

  /** `sha`'s position, given one if it hasn't got one, as unreachable and without parents. */
  private intern(sha: string): number {
    const known = this.find(sha);
    if (known !== -1) return known;
    if (this.count === this.firstParents.length) this.growArrays();
    if ((this.count + 1) * 2 > this.slots.length) this.growSlots();
    const id = this.count++;
    for (let byte = 0; byte < this.width; byte++) {
      this.shas[id * this.width + byte] = hexByte(sha, byte);
    }
    this.firstParents[id] = -1;
    this.place(id, this.hashOf(sha));
    return id;
  }

  private place(id: number, hash: number): void {
    const mask = this.slots.length - 1;
    let slot = hash & mask;
    while (this.slots[slot] !== 0) slot = (slot + 1) & mask;
    this.slots[slot] = id + 1;
  }

  /** A SHA's first four bytes: random enough to hash it by. */
  private hashOf(sha: string): number {
    return (
      (hexByte(sha, 0) << 24) | (hexByte(sha, 1) << 16) | (hexByte(sha, 2) << 8) | hexByte(sha, 3)
    );
  }

  private matches(id: number, sha: string): boolean {
    for (let byte = 0; byte < this.width; byte++) {
      if (this.shas[id * this.width + byte] !== hexByte(sha, byte)) return false;
    }
    return true;
  }

  private growArrays(): void {
    const capacity = this.firstParents.length * 2;
    this.shas = grown(this.shas, capacity * this.width);
    this.firstParents = grown(this.firstParents, capacity);
    this.reachable = grown(this.reachable, capacity);
  }

  private growSlots(): void {
    this.slots = new Int32Array(this.slots.length * 2);
    for (let id = 0; id < this.count; id++) {
      const at = id * this.width;
      const s = this.shas;
      this.place(id, (s[at]! << 24) | (s[at + 1]! << 16) | (s[at + 2]! << 8) | s[at + 3]!);
    }
  }
}
