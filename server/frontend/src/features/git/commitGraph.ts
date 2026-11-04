import type { GitCommit } from './types';

interface GraphLane { color: string; expected: string }
interface GraphLine { lane: number; color: string }
interface GraphEdge { from: number; to: number; color: string; startsAtNode: boolean }
export interface CommitGraphRow {
  lane: number;
  color: string;
  incoming: GraphLine[];
  edges: GraphEdge[];
  continuing: GraphLine[];
  columns: number;
}

const graphLaneColors = ['#4daafc', '#e5c07b', '#c586c0', '#4ec9b0', '#ce9178', '#b4a0ff', '#89d185', '#f48771'];

// The log starts at the selected repository's HEAD and is topologically sorted.
// Keep each first parent in its child's lane so HEAD's mainline stays at column 0.
export function buildCommitGraph(commits: readonly Pick<GitCommit, 'id' | 'parents'>[]) {
  const active: (GraphLane | undefined)[] = [];
  const rows = new Map<string, CommitGraphRow>();
  let nextColor = 0;
  const allocateLane = (expected: string, usedColors: Set<string>): GraphLane => {
    const availableOffset = graphLaneColors.findIndex((_, offset) => !usedColors.has(graphLaneColors[(nextColor + offset) % graphLaneColors.length]));
    if (availableOffset >= 0) nextColor += availableOffset;
    const color = graphLaneColors[nextColor++ % graphLaneColors.length];
    usedColors.add(color);
    return { color, expected };
  };
  const freeLane = () => {
    const empty = active.findIndex(lane => !lane);
    return empty < 0 ? active.length : empty;
  };

  for (const commit of commits) {
    const inputLanes = [...active];
    const usedColors = new Set(inputLanes.flatMap(lane => lane ? [lane.color] : []));
    const incoming = inputLanes.flatMap((lane, index) => lane?.expected === commit.id ? [{ lane: index, color: lane.color }] : []);
    const nodeLane = incoming[0]?.lane ?? freeLane();
    const lane = active[nodeLane] ?? allocateLane(commit.id, usedColors);
    for (const line of incoming) active[line.lane] = undefined;

    const edges: GraphEdge[] = inputLanes.flatMap((candidate, index) => candidate && candidate.expected !== commit.id
      ? [{ from: index, to: index, color: candidate.color, startsAtNode: false }] : []);
    commit.parents.forEach((parent, parentIndex) => {
      let target: number;
      if (parentIndex === 0) {
        // Do not merge lanes early when both already lead to this parent:
        // they meet at the parent's node, preserving the mainline's color.
        target = nodeLane;
        active[target] = { ...lane, expected: parent };
      } else {
        target = active.findIndex(candidate => candidate?.expected === parent);
        if (target < 0) {
          target = freeLane();
          active[target] = allocateLane(parent, usedColors);
        }
      }
      edges.push({ from: nodeLane, to: target, color: active[target]!.color, startsAtNode: true });
    });

    while (active.length && !active[active.length - 1]) active.pop();
    rows.set(commit.id, {
      lane: nodeLane, color: lane.color, incoming, edges,
      continuing: active.flatMap((candidate, index) => candidate ? [{ lane: index, color: candidate.color }] : []),
      columns: Math.max(nodeLane + 1, inputLanes.length, active.length),
    });
  }

  return rows;
}
