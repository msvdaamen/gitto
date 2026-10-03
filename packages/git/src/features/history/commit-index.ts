// The commits the history reaches, kept in the main process, so that what a change to the refs
// added to the history and took from it is known exactly, without git walking it all again.
import type { Repo } from "../../core/repo";

/** Each hex digit's value, by character code. */
const HEX = new Uint8Array(128);
for (let i = 0; i < 16; i++) HEX["0123456789abcdef".charCodeAt(i)] = i;

const SPACE = 0x20;
const NEWLINE = 0x0a;

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
 * about 10MB), and read from git's output as bytes, without making strings of them.
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
  /** The SHA being looked up, as bytes. */
  private readonly sha: Uint8Array;

  private constructor(
    /** Bytes in a SHA: 20, or 32 in a SHA-256 repository. */
    private readonly width: number,
    /** The commits it has those reachable from. */
    public tips: ReadonlySet<string>,
  ) {
    this.shas = new Uint8Array(this.firstParents.length * width);
    this.sha = new Uint8Array(width);
  }

  /**
   * The commits reachable from `tips`, read from git as it lists them, so a big history doesn't
   * hold up the main process.
   */
  static async load(repo: Repo, tips: ReadonlySet<string>, signal?: AbortSignal) {
    const [first] = tips;
    const index = new CommitIndex(first ? first.length / 2 : 20, tips);
    if (!first) return index;
    // A line cut off at the end of a chunk, to be read with the next.
    let rest: Buffer = Buffer.alloc(0);
    await repo.read(["rev-list", "--parents", "--stdin"], {
      signal,
      stdin: `${[...tips].join("\n")}\n`,
      onStdout: (chunk) => {
        const text = rest.length > 0 ? Buffer.concat([rest, chunk]) : chunk;
        const end = text.lastIndexOf(NEWLINE) + 1;
        index.defineLines(text, end);
        rest = text.subarray(end);
      },
    });
    index.defineLines(Buffer.concat([rest, Buffer.from("\n")]), rest.length + 1);
    return index;
  }

  /** How many commits it has, reachable or not. */
  get size(): number {
    return this.count;
  }

  /** Whether `sha` is reachable from the tips. */
  has(sha: string): boolean {
    this.parseSha(sha);
    const id = this.find();
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
    // Nothing is new from a commit that was reachable already, as neither are its parents: moving
    // a branch back, or making one at a commit there was, takes no asking git.
    const fresh = [...tips].filter((tip) => !from.has(tip) && !this.has(tip));
    const listed = fresh.length
      ? await repo.read(["rev-list", "--parents", "--stdin"], {
          signal,
          stdin: `${[...fresh, ...[...from].map((tip) => `^${tip}`)].join("\n")}\n`,
        })
      : "";
    if (this.tips !== from) return undefined;

    const text = Buffer.from(listed, "latin1");
    const added = new Set<string>();
    // Git lists commits reachable already when the clock was skewed.
    for (const id of this.defineLines(text, text.length, true)) added.add(this.shaOf(id));
    this.tips = tips;

    // A tip that's gone can still be reachable, e.g. a branch's old commit, now the parent of a new
    // one. Otherwise, what's reachable is worked out again.
    const kept = new Set(tips);
    for (const sha of added) {
      this.parseSha(sha);
      for (const parent of this.parentsOf(this.find())) kept.add(this.shaOf(parent));
    }
    const gone = [...from].filter((tip) => !kept.has(tip));
    return { added, lost: gone.length > 0 && this.markReachable() };
  }

  /** Marks what's reachable from the tips again; whether anything that was, isn't anymore. */
  private markReachable(): boolean {
    const marked = new Uint8Array(this.reachable.length);
    // Each commit goes on it once at most.
    const stack = new Int32Array(this.count);
    let top = 0;
    for (const tip of this.tips) {
      this.parseSha(tip);
      const id = this.find();
      if (id !== -1 && marked[id] === 0) {
        marked[id] = 1;
        stack[top++] = id;
      }
    }
    while (top > 0) {
      const id = stack[--top]!;
      const parent = this.firstParents[id]!;
      if (parent === -1) continue;
      if (marked[parent] === 0) {
        marked[parent] = 1;
        stack[top++] = parent;
      }
      const others = this.otherParents.get(id);
      if (!others) continue;
      for (const other of others) {
        if (marked[other] === 0) {
          marked[other] = 1;
          stack[top++] = other;
        }
      }
    }
    let lost = false;
    for (let id = 0; id < this.count && !lost; id++) {
      lost = this.reachable[id] === 1 && marked[id] === 0;
    }
    this.reachable = marked;
    return lost;
  }

  /**
   * Adds the commits `rev-list --parents` lists in `text` up to `end` (whole lines), as reachable,
   * and returns their positions; when `skipReachable`, but for those that were already.
   */
  private defineLines(text: Uint8Array, end: number, skipReachable = false): number[] {
    const hexWidth = this.width * 2;
    const defined: number[] = [];
    for (let at = 0; at + hexWidth <= end; at++) {
      this.readSha(text, at);
      at += hexWidth;
      let id = this.find();
      if (skipReachable && id !== -1 && this.reachable[id] === 1) {
        while (at < end && text[at] !== NEWLINE) at++;
        continue;
      }
      if (id === -1) id = this.intern();
      this.reachable[id] = 1;
      defined.push(id);
      let first = -1;
      let others: number[] | undefined;
      while (text[at] === SPACE) {
        this.readSha(text, at + 1);
        at += 1 + hexWidth;
        let parent = this.find();
        if (parent === -1) parent = this.intern();
        if (first === -1) first = parent;
        else (others ??= []).push(parent);
      }
      this.firstParents[id] = first;
      if (others) this.otherParents.set(id, others);
      else this.otherParents.delete(id);
    }
    return defined;
  }

  private parentsOf(id: number): number[] {
    if (id === -1 || this.firstParents[id] === -1) return [];
    return [this.firstParents[id]!, ...(this.otherParents.get(id) ?? [])];
  }

  private shaOf(id: number): string {
    let sha = "";
    for (let byte = 0; byte < this.width; byte++) {
      sha += this.shas[id * this.width + byte]!.toString(16).padStart(2, "0");
    }
    return sha;
  }

  /** Reads the hex SHA at `at` in `text` into `sha`. */
  private readSha(text: Uint8Array, at: number): void {
    for (let byte = 0; byte < this.width; byte++) {
      this.sha[byte] = (HEX[text[at + 2 * byte]!]! << 4) | HEX[text[at + 2 * byte + 1]!]!;
    }
  }

  /** Reads the hex SHA `hex` into `sha`. */
  private parseSha(hex: string): void {
    for (let byte = 0; byte < this.width; byte++) {
      this.sha[byte] = (HEX[hex.charCodeAt(2 * byte)]! << 4) | HEX[hex.charCodeAt(2 * byte + 1)]!;
    }
  }

  /** `sha`'s position; -1 if it hasn't got one. */
  private find(): number {
    const mask = this.slots.length - 1;
    for (let slot = this.hashOf(this.sha, 0) & mask; ; slot = (slot + 1) & mask) {
      const id = this.slots[slot]! - 1;
      if (id === -1 || this.matches(id)) return id;
    }
  }

  /** A position for `sha`, which hasn't got one, as unreachable and without parents. */
  private intern(): number {
    if (this.count === this.firstParents.length) this.growArrays();
    if ((this.count + 1) * 2 > this.slots.length) this.growSlots();
    const id = this.count++;
    this.shas.set(this.sha, id * this.width);
    this.firstParents[id] = -1;
    this.place(id, this.hashOf(this.sha, 0));
    return id;
  }

  private place(id: number, hash: number): void {
    const mask = this.slots.length - 1;
    let slot = hash & mask;
    while (this.slots[slot] !== 0) slot = (slot + 1) & mask;
    this.slots[slot] = id + 1;
  }

  /** The first four bytes of the SHA at `at` in `bytes`: random enough to hash it by. */
  private hashOf(bytes: Uint8Array, at: number): number {
    return (bytes[at]! << 24) | (bytes[at + 1]! << 16) | (bytes[at + 2]! << 8) | bytes[at + 3]!;
  }

  private matches(id: number): boolean {
    const at = id * this.width;
    for (let byte = 0; byte < this.width; byte++) {
      if (this.shas[at + byte] !== this.sha[byte]) return false;
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
    for (let id = 0; id < this.count; id++) this.place(id, this.hashOf(this.shas, id * this.width));
  }
}
