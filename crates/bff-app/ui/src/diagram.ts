/** Where each step sits in the workflow picture. A step is in the column after the last step it waits for, so steps
 *  that wait for the same step share a column: they can run at the same time. Each side has a lane, writers above
 *  second check, and steps in the same column and lane are stacked. */
export type Side = "writing" | "checking";
export type DiagramStep = { id: string; title: string; side: Side; needs?: string[] };
export type Node = { id: string; x: number; y: number; side: Side };
export type Edge = { from: string; to: string; x1: number; y1: number; x2: number; y2: number };

const COL = 150;
const LEFT = 104;
const TOP = 56;
const STEP = 122;
const GAP = 52;

export function layout(steps: DiagramStep[]): { nodes: Node[]; edges: Edge[]; width: number; height: number; lanes: Record<Side, number> } {
  const needs = (i: number) => steps[i].needs ?? (i > 0 ? [steps[i - 1].id] : []);
  const depth = new Map<string, number>();
  steps.forEach((s, i) => depth.set(s.id, Math.max(-1, ...needs(i).map((n) => depth.get(n) ?? -1)) + 1));
  const row = new Map<string, number>();
  const rows: Record<string, number> = {};
  const rowsIn: Record<Side, number> = { writing: 1, checking: 1 };
  for (const s of steps) {
    const key = `${depth.get(s.id)}:${s.side}`;
    const r = rows[key] ?? 0;
    rows[key] = r + 1;
    row.set(s.id, r);
    rowsIn[s.side] = Math.max(rowsIn[s.side], r + 1);
  }
  const lanes: Record<Side, number> = { writing: TOP, checking: TOP + rowsIn.writing * STEP + GAP };
  const nodes = steps.map((s) => ({ id: s.id, x: LEFT + depth.get(s.id)! * COL, y: lanes[s.side] + row.get(s.id)! * STEP, side: s.side }));
  const at = (id: string) => nodes.find((n) => n.id === id)!;
  const edges: Edge[] = [];
  steps.forEach((s, i) => {
    for (const from of needs(i)) {
      const a = at(from);
      const b = at(s.id);
      if (a && b) edges.push({ from, to: s.id, x1: a.x, y1: a.y, x2: b.x, y2: b.y });
    }
  });
  const cols = Math.max(0, ...nodes.map((n) => (n.x - LEFT) / COL));
  return { nodes, edges, width: LEFT * 2 + cols * COL, height: lanes.checking + rowsIn.checking * STEP + 20, lanes };
}

/** A title as at most three lines of about `max` characters, cut with an ellipsis when it is longer. */
export function wrap(title: string, max: number): string[] {
  const words = title.split(/\s+/).filter(Boolean);
  const lines: string[] = [];
  let cur = "";
  for (const w of words) {
    if (cur && (cur + " " + w).length > max) {
      lines.push(cur);
      cur = w;
    } else cur = cur ? cur + " " + w : w;
  }
  if (cur) lines.push(cur);
  if (lines.length > 3) {
    const kept = lines.slice(0, 3);
    kept[2] = kept[2].slice(0, Math.max(1, max - 1)).trimEnd() + "…";
    return kept;
  }
  return lines;
}

/** How many results a step has made, from the list of results (one entry for each version). */
export function runsOf(artifacts: { name: string }[], id: string): number {
  return artifacts.filter((a) => a.name === id).length;
}

/** What hovering a step says: its model, how long it took and what it cost, leaving out what is not known. */
export function detailLine(model: string | null, seconds: number | null, dollars: number | null, known: boolean): string {
  const parts: string[] = [];
  if (model) parts.push(model);
  if (seconds !== null) parts.push(seconds < 60 ? `${Math.round(seconds)} s` : `${Math.floor(seconds / 60)} min ${Math.round(seconds % 60)} s`);
  if (known && dollars !== null) parts.push(dollars < 0.01 ? "under $0.01" : `$${dollars.toFixed(2)}`);
  else if (!known) parts.push("cost unknown");
  return parts.join(" · ");
}
