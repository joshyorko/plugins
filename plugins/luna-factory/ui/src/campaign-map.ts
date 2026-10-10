// Wave layout for the Campaign Map. Pure functions over persisted graph/task data; no inference.
import { taskTone } from "./lunar";

export interface MapNode { id: string; title: string; dependencies: string[]; state: string; owner_thread: string | null }
export interface WaveLayout {
  waves: string[][];
  depth: Map<string, number>;
  dependents: Map<string, string[]>;
  /** Prerequisites that name no task in this graph; shown as text, never drawn. */
  missing: Map<string, string[]>;
  /** Not-done tasks whose known prerequisites are all done. Derived; the server state stays primary. */
  prerequisitesMet: Set<string>;
  /** Longest chain of not-done tasks by count. No durations exist, so this is not a time estimate. */
  remainingChain: string[];
  cyclic: boolean;
}

export function layoutWaves(nodes: MapNode[]): WaveLayout {
  const byId = new Map(nodes.map(node => [node.id, node]));
  const dependents = new Map<string, string[]>(nodes.map(node => [node.id, []]));
  const missing = new Map<string, string[]>();
  for (const node of nodes) {
    for (const dependency of node.dependencies) {
      if (byId.has(dependency)) dependents.get(dependency)?.push(node.id);
      else missing.set(node.id, [...(missing.get(node.id) ?? []), dependency]);
    }
  }
  const depth = new Map<string, number>();
  const visiting = new Set<string>();
  let cyclic = false;
  const visit = (id: string): number => {
    const known = depth.get(id);
    if (known !== undefined) return known;
    if (visiting.has(id)) { cyclic = true; return 0; }
    visiting.add(id);
    const node = byId.get(id);
    let value = 0;
    for (const dependency of node?.dependencies ?? []) if (byId.has(dependency)) value = Math.max(value, visit(dependency) + 1);
    visiting.delete(id);
    depth.set(id, value);
    return value;
  };
  for (const node of nodes) visit(node.id);
  const waves: string[][] = [];
  for (const node of nodes) (waves[depth.get(node.id) ?? 0] ??= []).push(node.id);
  // One barycenter pass keeps related tasks near each other without moving the first wave.
  const order = new Map<string, number>();
  waves.forEach((wave, index) => {
    if (index > 0) {
      const weight = (id: string) => {
        const positions = (byId.get(id)?.dependencies ?? []).map(dependency => order.get(dependency)).filter((value): value is number => value !== undefined);
        return positions.length ? positions.reduce((sum, value) => sum + value, 0) / positions.length : Number.MAX_SAFE_INTEGER;
      };
      wave.sort((a, b) => weight(a) - weight(b));
    }
    wave.forEach((id, position) => order.set(id, position));
  });
  const done = (id: string) => taskTone(byId.get(id)?.state ?? "") === "done";
  const prerequisitesMet = new Set(nodes.filter(node => !done(node.id) && node.dependencies.filter(id => byId.has(id)).every(done)).map(node => node.id));
  const chainFrom = new Map<string, string[]>();
  const longest = (id: string, trail: Set<string>): string[] => {
    const cached = chainFrom.get(id);
    if (cached) return cached;
    if (trail.has(id)) return [];
    trail.add(id);
    let best: string[] = [];
    for (const next of dependents.get(id) ?? []) {
      if (done(next)) continue;
      const candidate = longest(next, trail);
      if (candidate.length > best.length) best = candidate;
    }
    trail.delete(id);
    const result = [id, ...best];
    chainFrom.set(id, result);
    return result;
  };
  let remainingChain: string[] = [];
  for (const node of nodes) {
    if (done(node.id) || !prerequisitesMet.has(node.id)) continue;
    const chain = longest(node.id, new Set());
    if (chain.length > remainingChain.length) remainingChain = chain;
  }
  return { waves, depth, dependents, missing, prerequisitesMet, remainingChain: remainingChain.length > 1 ? remainingChain : [], cyclic };
}

export interface MapGeometry { nodeWidth: number; nodeHeight: number; columnGap: number; rowGap: number; padX: number; padY: number }
export const MAP_GEOMETRY: MapGeometry = { nodeWidth: 216, nodeHeight: 68, columnGap: 76, rowGap: 18, padX: 20, padY: 22 };

/** Shrinks columns to fit the pane when it can (down to readable minimums); wider graphs pan instead. */
export function fitGeometry(waveCount: number, available?: number): MapGeometry {
  const base = MAP_GEOMETRY;
  const columns = Math.max(1, waveCount);
  if (!available || available <= 0) return base;
  const natural = base.padX * 2 + columns * base.nodeWidth + (columns - 1) * base.columnGap;
  if (natural <= available) return base;
  const columnGap = Math.max(36, Math.min(base.columnGap, Math.floor((available - base.padX * 2) / columns * 0.22)));
  const nodeWidth = Math.floor((available - base.padX * 2 - (columns - 1) * columnGap) / columns);
  return nodeWidth >= 156 ? { ...base, columnGap, nodeWidth: Math.min(base.nodeWidth, nodeWidth) } : { ...base, nodeWidth: 156, columnGap: 36 };
}

export function nodePosition(layout: WaveLayout, id: string, g: MapGeometry = MAP_GEOMETRY): { x: number; y: number } | null {
  const wave = layout.depth.get(id);
  if (wave === undefined) return null;
  const row = layout.waves[wave]?.indexOf(id) ?? -1;
  if (row < 0) return null;
  return { x: g.padX + wave * (g.nodeWidth + g.columnGap), y: g.padY + row * (g.nodeHeight + g.rowGap) };
}

export function canvasSize(layout: WaveLayout, g: MapGeometry = MAP_GEOMETRY): { width: number; height: number } {
  const columns = Math.max(1, layout.waves.length);
  const rows = Math.max(1, ...layout.waves.map(wave => wave.length));
  return { width: g.padX * 2 + columns * g.nodeWidth + (columns - 1) * g.columnGap, height: g.padY * 2 + rows * g.nodeHeight + (rows - 1) * g.rowGap };
}

/** Smooth dependency curve from a prerequisite's right edge to a dependent's left edge. */
export function edgePath(from: { x: number; y: number }, to: { x: number; y: number }, g: MapGeometry = MAP_GEOMETRY): string {
  const x1 = from.x + g.nodeWidth;
  const y1 = from.y + g.nodeHeight / 2;
  const x2 = to.x;
  const y2 = to.y + g.nodeHeight / 2;
  const bend = Math.max(18, (x2 - x1) * 0.5);
  return `M${x1} ${y1} C${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
}
