export interface Pt { x: number; y: number }
export interface Cubic { a: Pt; b: Pt; c: Pt; d: Pt }
export interface Port { id: string; x: number; y: number; side?: 'left' | 'right'; ry?: number }
export interface Obstacle { id: string; x: number; y: number; width: number; height: number }
export interface Strand { id: string; trunk: number; branch: Cubic[] }
export interface ThreadTree {
  origin: Pt;
  trunks: Cubic[];
  strands: Strand[];
  right: Cubic[];
}

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

export const trunkCount = (n: number): number => (n <= 2 ? 1 : n <= 5 ? 2 : 3);

const ease = (a: Pt, d: Pt, pull = 0.45): Cubic => {
  const dx = d.x - a.x;
  return { a, b: { x: a.x + dx * pull, y: a.y }, c: { x: d.x - dx * pull, y: d.y }, d };
};

export const cubicPoint = ({ a, b, c, d }: Cubic, t: number): Pt => {
  const u = 1 - t;
  const w0 = u * u * u, w1 = 3 * u * u * t, w2 = 3 * u * t * t, w3 = t * t * t;
  return { x: w0 * a.x + w1 * b.x + w2 * c.x + w3 * d.x, y: w0 * a.y + w1 * b.y + w2 * c.y + w3 * d.y };
};

export const branchCurve = (fork: Pt, port: Port, stiff = 0.5): Cubic => {
  const end = { x: port.x, y: port.y };
  const span = Math.max(36, Math.abs(end.x - fork.x));
  const outward = end.x < fork.x ? -1 : 1;
  const into = port.side === 'left' ? -1 : 1;
  return { a: fork, b: { x: fork.x + outward * span * stiff, y: fork.y }, c: { x: end.x + into * span * stiff, y: end.y }, d: end };
};

const forkGap = (origin: Pt, edge: number) => Math.max(40, Math.min(120, (origin.x - edge) * 0.3));

export const threadTree = (lock: Pt, radius: number, ports: Port[], target: { x: number; y: number; h: number } | null, _obstacles: Obstacle[] = [], edge?: number): ThreadTree => {
  const origin = { x: lock.x - radius, y: lock.y };
  const restY = (p: Port) => p.ry ?? p.y;
  const sorted = [...ports].sort((p, q) => restY(p) - restY(q) || p.id.localeCompare(q.id));
  const groups = trunkCount(sorted.length);
  const size = Math.ceil(sorted.length / Math.max(1, groups));
  const right = edge ?? (ports.length ? Math.max(...ports.map((p) => p.x)) : origin.x - 200);
  const forkX = origin.x - forkGap(origin, Math.min(right, origin.x - 60));
  const trunks: Cubic[] = [];
  const strands: Strand[] = [];
  for (let g = 0; g * size < sorted.length; g += 1) {
    const members = sorted.slice(g * size, (g + 1) * size);
    const meanY = members.reduce((sum, p) => sum + restY(p), 0) / members.length;
    const fork = { x: forkX, y: lerp(origin.y, meanY, 0.62) };
    trunks.push(ease(origin, fork, 0.5));
    for (const port of members) strands.push({ id: port.id, trunk: trunks.length - 1, branch: [branchCurve(fork, port)] });
  }
  const bundle = target
    ? [-1, 0, 1].map((k) => {
      const from = { x: lock.x + radius, y: lock.y };
      const to = { x: target.x, y: target.y + k * 3 };
      const q = ease(from, to, 0.5);
      return { ...q, b: { ...q.b, y: q.b.y + 6 + k }, c: { ...q.c, y: q.c.y + 6 - k } };
    })
    : [];
  return { origin, trunks, strands, right: bundle };
};

const r1 = (n: number) => Math.round(n * 10) / 10;
export const pathD = (cubics: Cubic[]): string => cubics.map((q, i) => `${i ? '' : `M${r1(q.a.x)} ${r1(q.a.y)}`}C${r1(q.b.x)} ${r1(q.b.y)} ${r1(q.c.x)} ${r1(q.c.y)} ${r1(q.d.x)} ${r1(q.d.y)}`).join('');

export const sampleStrand = (tree: ThreadTree, id: string, n = 12): Pt[] => {
  const strand = tree.strands.find((s) => s.id === id);
  if (!strand) return [];
  const trunk = tree.trunks[strand.trunk]!;
  const reversed = [...strand.branch].reverse().map((q) => ({ a: q.d, b: q.c, c: q.b, d: q.a }));
  const path = [...reversed, { a: trunk.d, b: trunk.c, c: trunk.b, d: trunk.a }];
  const points: Pt[] = [];
  for (let i = 0; i < n; i += 1) {
    const t = (i / (n - 1)) * path.length;
    const k = Math.min(path.length - 1, Math.floor(t));
    points.push(cubicPoint(path[k]!, t - k));
  }
  return points;
};


const line = (a: Pt, d: Pt): Cubic => ({ a, b: { x: lerp(a.x, d.x, 1 / 3), y: lerp(a.y, d.y, 1 / 3) }, c: { x: lerp(a.x, d.x, 2 / 3), y: lerp(a.y, d.y, 2 / 3) }, d });
const backwards = (q: Cubic): Cubic => ({ a: q.d, b: q.c, c: q.b, d: q.a });

export const strandRoute = (tree: ThreadTree, id: string, center: Pt): Cubic[] => {
  const strand = tree.strands.find((s) => s.id === id);
  if (!strand) return [];
  return [...[...strand.branch].reverse().map(backwards), backwards(tree.trunks[strand.trunk]!), line(tree.origin, center)];
};

export const slotRoute = (tree: ThreadTree, center: Pt, radius: number, slot: Pt): Cubic[] => {
  const mid = tree.right[1] ?? tree.right[0];
  if (!mid) return [line(center, { x: center.x + radius, y: center.y }), ease({ x: center.x + radius, y: center.y }, slot)];
  return [line(center, mid.a), mid, ease(mid.d, slot, 0.5)];
};

export interface Trace { points: Pt[]; marks: number[] }

export const traceEven = (route: Cubic[], n = 24): Trace => {
  if (!route.length) return { points: [], marks: [] };
  const STEPS = 48;
  const dense: Array<{ p: Pt; s: number }> = [{ p: route[0]!.a, s: 0 }];
  const ends: number[] = [];
  let s = 0;
  for (const q of route) {
    let last = q.a;
    for (let i = 1; i <= STEPS; i += 1) {
      const p = cubicPoint(q, i / STEPS);
      s += Math.hypot(p.x - last.x, p.y - last.y);
      dense.push({ p, s });
      last = p;
    }
    ends.push(s);
  }
  const total = s || 1;
  const points: Pt[] = [];
  let j = 0;
  for (let i = 0; i < n; i += 1) {
    const at = (i / Math.max(1, n - 1)) * total;
    while (j < dense.length - 2 && dense[j + 1]!.s < at) j += 1;
    const a = dense[j]!;
    const b = dense[j + 1] ?? a;
    const f = b.s > a.s ? Math.min(1, (at - a.s) / (b.s - a.s)) : 0;
    points.push({ x: lerp(a.p.x, b.p.x, f), y: lerp(a.p.y, b.p.y, f) });
  }
  return { points, marks: ends.map((e) => e / total) };
};
