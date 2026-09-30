/**
 * Lays commits (newest first, in topological order) out in lanes. Returns one glyph per lane for
 * every commit: `●` for the commit itself, `│` for lanes passing by. Merge and fork connectors
 * aren't drawn yet.
 */
export function computeGraph(commits: { sha: string; parents: string[] }[]): string[][] {
  // The commit each lane is heading towards; `null` for a free lane.
  const lanes: (string | null)[] = [];

  return commits.map(({ sha, parents }) => {
    let column = lanes.indexOf(sha);
    if (column === -1) column = freeLane(lanes);
    // Other lanes heading here (branches forked from this commit) end at this row.
    for (let i = 0; i < lanes.length; i++) if (lanes[i] === sha) lanes[i] = null;
    lanes[column] = sha;

    const cells = lanes.map((lane, i) => (i === column ? "●" : lane ? "│" : " "));

    lanes[column] = parents[0] ?? null;
    for (const parent of parents.slice(1)) {
      if (!lanes.includes(parent)) lanes[freeLane(lanes)] = parent;
    }
    while (lanes.length > 0 && lanes.at(-1) === null) lanes.pop();

    return cells;
  });
}

function freeLane(lanes: (string | null)[]): number {
  const free = lanes.indexOf(null);
  return free === -1 ? lanes.length : free;
}
