/** A line in one row of the graph, from lane `from` to lane `to` (the same lane for a straight one). */
export interface GraphEdge {
  from: number;
  to: number;
  /** Part of the dashed line from the uncommitted changes to the commit they're based on. */
  dashed?: true;
}

/**
 * One commit's row of the graph. Each row is drawn on its own: lines in the top half end at the
 * commit's node, lines in the bottom half start there, and lines passing by run top to bottom.
 */
export interface GraphRow {
  /** The lane the commit's node sits in; also its colour. */
  column: number;
  /** Lanes passing by the node, top to bottom (`from` and `to` are the same lane). */
  through: GraphEdge[];
  /**
   * Lines from the top of the row into the node: its own lane, and branches forked from it. Each
   * takes the colour of the lane it comes from.
   */
  top: GraphEdge[];
  /**
   * Lines from the node to the bottom of the row: to its first parent, and to merged parents. Each
   * takes the colour of the lane it goes to, so a merge is drawn in the merged branch's colour.
   */
  bottom: GraphEdge[];
  /** How many lanes the row uses. */
  width: number;
}

/** A commit to lay out. `dashed` draws the line to its parent dashed, for uncommitted changes. */
export interface GraphCommit {
  sha: string;
  parents: string[];
  dashed?: boolean;
}

/**
 * Lays commits (newest first; children always before their parents) out in lanes, the way
 * GitKraken does: a branch keeps its lane from its tip down to where it forked off, merges curve
 * in from the merged branch's lane, and forks curve out of the commit they started at.
 */
export function computeGraph(commits: GraphCommit[]): GraphRow[] {
  // The commit each lane is heading towards; `null` for a free lane.
  const lanes: (string | null)[] = [];
  // Lanes drawn dashed, from a `dashed` commit down to its parent.
  const dashed = new Set<number>();

  return commits.map((commit) => {
    const { sha, parents } = commit;
    const incoming = lanes.flatMap((lane, i) => (lane === sha ? [i] : []));
    // A branch tip nothing leads to yet starts in the first free lane.
    const column = incoming[0] ?? freeLane(lanes);
    const through = lanes.flatMap((lane, i) =>
      lane !== null && lane !== sha ? [edge(i, i, dashed.has(i))] : [],
    );
    const top = incoming.map((lane) => edge(lane, column, dashed.has(lane)));

    // Branches forked from this commit end here. Their lanes free up only after this row, so a
    // merged parent below doesn't reuse a lane that is still drawn above the node.
    for (const lane of incoming) {
      lanes[lane] = null;
      dashed.delete(lane);
    }
    const bottom: GraphEdge[] = [];
    const [first, ...merged] = parents;
    if (first !== undefined) {
      lanes[column] = first;
      if (commit.dashed) dashed.add(column);
      bottom.push(edge(column, column, !!commit.dashed));
    }
    for (const parent of merged) {
      let lane = lanes.indexOf(parent);
      if (lane === -1) {
        lane = freeLane(lanes, incoming);
        lanes[lane] = parent;
      }
      bottom.push(edge(column, lane, false));
    }
    while (lanes.length > 0 && lanes.at(-1) === null) lanes.pop();

    const used = [column, ...incoming, ...[...through, ...bottom].map((line) => line.to)];
    return { column, through, top, bottom, width: Math.max(...used) + 1 };
  });
}

function edge(from: number, to: number, dashed: boolean): GraphEdge {
  return dashed ? { from, to, dashed: true } : { from, to };
}

function freeLane(lanes: (string | null)[], reserved: number[] = []): number {
  const free = lanes.findIndex((lane, i) => lane === null && !reserved.includes(i));
  return free === -1 ? lanes.length : free;
}
