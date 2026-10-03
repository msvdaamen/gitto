import { Index, Match, Show, Switch, type JSX } from "solid-js";

import { Avatar } from "@/components/ui/avatar";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { GraphEdge, GraphRow } from "@/git/graph";

/** Width of a lane, and the space left and right of the lanes. */
const LANE = 20;
const PADDING = 6;
/** Height of a history row, including its 1px bottom border. */
export const ROW_HEIGHT = 38;
/** Height of a row's graph: the row, without its bottom border. */
const HEIGHT = ROW_HEIGHT - 1;
const MIDDLE = HEIGHT / 2;
const NODE_RADIUS = 8;
/** Radius of the rounded corner where a line turns from one lane towards another. */
const CORNER = 8;

const LANE_COLORS = [
  "var(--primary-strong)",
  "var(--blue)",
  "var(--mint)",
  "var(--amber)",
  "var(--coral)",
];

/** The colour of a lane, shared by its line, its nodes and the labels of branches on it. */
export function laneColor(lane: number): string {
  return LANE_COLORS[lane % LANE_COLORS.length]!;
}

/** Width of the graph column for rows that use at most `lanes` lanes. */
export function graphWidth(lanes: number): number {
  return Math.min(Math.max(lanes * LANE + PADDING * 2, 64), 260);
}

function x(lane: number): number {
  return PADDING + lane * LANE + LANE / 2;
}

/** A lane passing by the node, from the top of the row to the bottom. */
function throughPath(edge: GraphEdge): string {
  return `M${x(edge.from)} 0V${HEIGHT + 1}`;
}

/** A line from the top of the row into the node: straight down, then a corner into the node. */
function topPath(edge: GraphEdge): string {
  const from = x(edge.from);
  const to = x(edge.to);
  if (from === to) return `M${from} 0V${MIDDLE}`;
  const towards = Math.sign(to - from);
  return `M${from} 0V${MIDDLE - CORNER}Q${from} ${MIDDLE} ${from + towards * CORNER} ${MIDDLE}H${to}`;
}

/** A line from the node to the bottom of the row: across to its lane, then a corner down. */
function bottomPath(edge: GraphEdge): string {
  const from = x(edge.from);
  const to = x(edge.to);
  // One past the row, over its bottom border, so the line meets the next row's.
  const bottom = HEIGHT + 1;
  if (from === to) return `M${from} ${MIDDLE}V${bottom}`;
  const towards = Math.sign(to - from);
  return `M${from} ${MIDDLE}H${to - towards * CORNER}Q${to} ${MIDDLE} ${to} ${MIDDLE + CORNER}V${bottom}`;
}

/**
 * A commit's row of the history graph. Commits are avatars ringed in their lane's colour, merges
 * small dots, the uncommitted changes a dashed, hollow circle with a dashed line to HEAD, and a
 * stash a box with a dashed line to the commit it was made on. A commit's node shows its author when
 * hovered.
 */
export function HistoryGraph(props: {
  row: GraphRow;
  author?: string;
  initials?: string;
  avatarColor?: string;
  wip?: boolean;
  stash?: boolean;
  /** Leave out the lines, e.g. while searching, when the rows they lead to may be hidden. */
  nodeOnly?: boolean;
}) {
  const node = () => x(props.row.column);
  const color = () => (props.wip || props.stash ? "var(--amber)" : laneColor(props.row.column));
  const isMerge = () => props.row.bottom.length > 1;

  return (
    <svg
      class="block overflow-visible"
      width="100%"
      height={HEIGHT}
      aria-hidden="true"
      fill="none"
      stroke-width="2"
    >
      <Show when={!props.nodeOnly}>
        {/* By position: the edges are new objects whenever the history is refetched, and `<For>`
            would re-create every path for them. */}
        <Index each={props.row.through}>
          {(edge) => <Line d={throughPath(edge())} edge={edge()} lane={edge().from} />}
        </Index>
        <Index each={props.row.top}>
          {(edge) => <Line d={topPath(edge())} edge={edge()} lane={edge().from} />}
        </Index>
        <Index each={props.row.bottom}>
          {(edge) => <Line d={bottomPath(edge())} edge={edge()} lane={edge().to} />}
        </Index>
      </Show>
      <Switch
        fallback={
          <AuthorTooltip author={props.author} initials={props.initials} color={props.avatarColor}>
            <Show
              when={!isMerge()}
              fallback={<circle cx={node()} cy={MIDDLE} r={4.5} fill={color()} stroke="none" />}
            >
              <circle
                cx={node()}
                cy={MIDDLE}
                r={NODE_RADIUS}
                fill={props.avatarColor ?? color()}
                stroke={color()}
              />
              <text
                x={node()}
                y={MIDDLE}
                fill="#fff"
                stroke="none"
                font-size="7"
                font-weight="700"
                text-anchor="middle"
                dominant-baseline="central"
              >
                {props.initials}
              </text>
            </Show>
          </AuthorTooltip>
        }
      >
        <Match when={props.wip}>
          <circle
            cx={node()}
            cy={MIDDLE}
            r={NODE_RADIUS - 1}
            fill="var(--bg)"
            stroke={color()}
            stroke-dasharray="3 2"
          />
        </Match>
        <Match when={props.stash}>
          {/* A box with its lid. */}
          <g stroke={color()} stroke-width="1.5">
            <rect
              x={node() - NODE_RADIUS + 1}
              y={MIDDLE - NODE_RADIUS + 1}
              width={2 * NODE_RADIUS - 2}
              height={2 * NODE_RADIUS - 2}
              rx={3}
              fill="var(--bg)"
            />
            <path d={`M${node() - NODE_RADIUS + 1} ${MIDDLE - 2}H${node() + NODE_RADIUS - 1}`} />
          </g>
        </Match>
      </Switch>
    </svg>
  );
}

/** Shows a commit's author in a tooltip while its node, `children`, is hovered. */
function AuthorTooltip(props: {
  author?: string;
  initials?: string;
  color?: string;
  children: JSX.Element;
}) {
  return (
    <Tooltip>
      <TooltipTrigger as="g" class="cursor-default">
        {props.children}
      </TooltipTrigger>
      <TooltipContent class="flex items-center gap-2 py-1 pl-1 pr-2.5">
        <Avatar initials={props.initials ?? ""} color={props.color} />
        <span class="truncate font-[600]">{props.author}</span>
      </TooltipContent>
    </Tooltip>
  );
}

/**
 * A line in `lane`'s colour; dashed amber on its way from the uncommitted changes to HEAD, or from a
 * stash to the commit it was made on.
 */
function Line(props: { d: string; edge: GraphEdge; lane: number }) {
  return (
    <path
      d={props.d}
      stroke={props.edge.dashed ? "var(--amber)" : laneColor(props.lane)}
      stroke-dasharray={props.edge.dashed ? "3 3" : undefined}
    />
  );
}
