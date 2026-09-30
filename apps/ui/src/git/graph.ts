/** A line in one row of the graph, from lane `from` to lane `to` (the same lane for a straight one). */
export interface GraphEdge {
  from: number;
  to: number;
  /** The lane whose colour the line takes. */
  color: number;
}

/**
 * One commit's row of the graph. Each row is drawn on its own: lines in the top half end at the
 * commit's node, lines in the bottom half start there, and lines passing by run top to bottom.
 */
export interface GraphRow {
  /** The lane the commit's node sits in; also its colour. */
  column: number;
  /** Lanes passing by the node, top to bottom. */
  through: number[];
  /** Lines from the top of the row into the node: its own lane, and branches forked from it. */
  top: GraphEdge[];
  /** Lines from the node to the bottom of the row: to its first parent, and to merged parents. */
  bottom: GraphEdge[];
  /** How many lanes the row uses. */
  width: number;
}

/**
 * Lays commits (newest first; children always before their parents) out in lanes, the way
 * GitKraken does: a branch keeps its lane from its tip down to where it forked off, merges curve
 * in from the merged branch's lane, and forks curve out of the commit they started at.
 */
export function computeGraph(commits: { sha: string; parents: string[] }[]): GraphRow[] {
  // The commit each lane is heading towards; `null` for a free lane.
  const lanes: (string | null)[] = [];

  return commits.map(({ sha, parents }) => {
    const incoming = lanes.flatMap((lane, i) => (lane === sha ? [i] : []));
    // A branch tip nothing leads to yet starts in the first free lane.
    const column = incoming[0] ?? freeLane(lanes);
    const through = lanes.flatMap((lane, i) => (lane !== null && lane !== sha ? [i] : []));
    const top = incoming.map((lane) => ({ from: lane, to: column, color: lane }));

    // Branches forked from this commit end here. Their lanes free up only after this row, so a
    // merged parent below doesn't reuse a lane that is still drawn above the node.
    for (const lane of incoming) lanes[lane] = null;
    const bottom: GraphEdge[] = [];
    const [first, ...merged] = parents;
    if (first !== undefined) {
      lanes[column] = first;
      bottom.push({ from: column, to: column, color: column });
    }
    for (const parent of merged) {
      let lane = lanes.indexOf(parent);
      if (lane === -1) {
        lane = freeLane(lanes, incoming);
        lanes[lane] = parent;
      }
      bottom.push({ from: column, to: lane, color: lane });
    }
    while (lanes.length > 0 && lanes.at(-1) === null) lanes.pop();

    const used = [column, ...through, ...incoming, ...bottom.map((edge) => edge.to)];
    return { column, through, top, bottom, width: Math.max(...used) + 1 };
  });
}

function freeLane(lanes: (string | null)[], reserved: number[] = []): number {
  const free = lanes.findIndex((lane, i) => lane === null && !reserved.includes(i));
  return free === -1 ? lanes.length : free;
}
