import { COLUMN_SNAP, FULL_SNAP, GRID_SNAP, ROW_SNAP, TOP_SNAP, type StageGroup, type StageState } from './state';

export const STRIP_W = 184;
export const THUMB_W = 132;
export const THUMB_X = 24;
export const GAP = 16;
export const STRIP_GAP = 16;
export const NARROW_W = 860;
export const TILT = 38;
export const PERSPECTIVE = 1100;
export const STRIP_PERSPECTIVE = 280;
export const DEFAULT_W = 560;
const DEFAULT_H = 360;
const PIP_W = 150;
const PIP_LAYOUT_W = 300;
const PIP_INSET = 10;
export const TAB_H = 44;
export const CLASSIC_TWO_COL = 900;
const THUMB_CROP = 1.1;
export const MIN_H = 160;
export const SPAN_MIN = 0.5;
export const SPLIT_MIN = 0.2;

export const SINGLE = [0.66, 0.84, 1] as const;
export const PAIR = [[1, 1], [2, 1], [1, 2]] as const;

export type Mode = 'stage' | 'strip' | 'classic' | 'behind' | 'pip' | 'hidden';
export type SideRole = 'sym' | 'seam' | 'outer';
export interface Sides { left: SideRole; right: SideRole }
export type Column = 'full' | 'main' | 'side' | 'float';

export interface Placement {
  x: number; y: number; w: number; h: number;
  scale: number; tilt: number; z: number; opacity: number;
  mode: Mode;
  pad?: number;
  sides?: Sides;
  crop?: boolean;
  fill?: boolean;
  extra?: number;
}
export interface Rect { x: number; y: number; w: number; h: number; pad?: number }

export interface LayoutInput {
  width: number;
  height: number;
  state: StageState;
  cards: readonly string[];
  contentH: Readonly<Record<string, number>>;
  lastW: Readonly<Record<string, number>>;
  pip: ReadonlySet<string>;
  view?: 'stage' | 'classic';
  columns?: Readonly<Record<string, Column>>;
  stripOpen: boolean;
  front?: string;
  grow?: ReadonlyMap<string, number>;
}
export interface Layout {
  placements: Map<string, Placement>;
  narrow: boolean;
  classic: boolean;
  stripShown: boolean;
  stripAuto: boolean;
  stageX: number;
  canvasH: number;
  tabs: string[];
}


export const ratioRange = (count: number, snap: number): readonly [number, number] => {
  if (count === 1) return [0.4, 1];
  if (count === 2) return [0.25, 0.75];
  if (count === 3) return snap === TOP_SNAP ? [0.25, 0.75] : [0.4, 0.75];
  return snap === COLUMN_SNAP ? [0.4, 0.75] : [0.25, 0.75];
};
export const defaultRatio = (count: number, snap: number): number => {
  if (count === 1) return SINGLE[Math.min(snap, 1)];
  if (count === 2) { const [a, b] = PAIR[snap] ?? PAIR[0]; return a / (a + b); }
  if (count === 4) return snap === COLUMN_SNAP ? 0.6 : 1 / 2;
  return 2 / 3;
};
export const ratioPresets = (count: number, snap: number): number[] => {
  if (count === 1) return [SINGLE[0], SINGLE[1], 1];
  if (count === 2) return PAIR.map(([a, b]) => a / (a + b));
  if (count === 3) return snap === TOP_SNAP ? [1 / 3, 1 / 2, 2 / 3] : [1 / 2, 2 / 3];
  return snap === COLUMN_SNAP ? [1 / 2, 0.6, 2 / 3] : [1 / 3, 1 / 2, 2 / 3];
};
export const SPLIT_PRESETS = [1 / 3, 1 / 2, 2 / 3] as const;
export const splitsOf = (g: Pick<StageGroup, 'splits'>): [number, number] => {
  const [a, b] = g.splits ?? [];
  if (a == null || b == null || !(a >= SPLIT_MIN - 1e-9) || !(b - a >= SPLIT_MIN - 1e-9) || !(b <= 1 - SPLIT_MIN + 1e-9)) return [1 / 3, 2 / 3];
  return [a, b];
};
export const ratioOf = (g: StageGroup): number => {
  const [lo, hi] = ratioRange(g.ids.length, g.snap);
  return Math.min(hi, Math.max(lo, g.ratio ?? defaultRatio(g.ids.length, g.snap)));
};
export const spanOf = (g: StageGroup): number => (g.ids.length < 2 ? 1 : Math.min(1, Math.max(SPAN_MIN, g.span ?? 1)));

const r1 = (n: number) => Math.round(n * 10) / 10;
const pinned = (input: LayoutInput, id: string) => input.state.stage.heights?.[id];
const heightOf = (input: LayoutInput, id: string, cap: number, hardCap = cap) => {
  const user = pinned(input, id);
  if (user != null) return Math.min(hardCap, Math.max(Math.min(MIN_H, hardCap), user));
  return Math.min(cap, Math.max(120, input.contentH[id] ?? DEFAULT_H));
};
const at = (x: number, y: number, w: number, h: number, mode: Mode, z = 3): Placement => ({ x: r1(x), y: r1(y), w: r1(w), h: r1(h), scale: 1, tilt: 0, z, opacity: 1, mode });

type StageRect = Rect & { mode: Mode; sides?: Sides; extra?: number };

const grows = (input: LayoutInput, id: string) => !!input.grow?.has(id) && pinned(input, id) == null;
const floorOf = (input: LayoutInput, id: string, nat: number) => (grows(input, id) ? nat - (input.grow?.get(id) ?? 0) : nat);
const share = (nat: readonly number[], fixed: readonly boolean[], grow: readonly boolean[], room: number, floor: readonly number[] = nat): number[] => {
  const sum = nat.reduce((a, b) => a + b, 0);
  if (sum <= room) {
    const n = grow.filter(Boolean).length;
    return nat.map((h, i) => (n && grow[i] ? h + (room - sum) / n : h));
  }
  const give = nat.map((h, i) => (grow[i] && !fixed[i] ? Math.max(0, h - floor[i]) : 0));
  const can = give.reduce((a, b) => a + b, 0);
  if (can < sum - room) return [...nat];
  const k = (sum - room) / can;
  return nat.map((h, i) => h - give[i] * k);
};
type Box = { h: number; extra: number };
const fitColumn = (input: LayoutInput, ids: readonly string[], room: number, hardCap = room): Box[] => {
  const nat = ids.map((id) => heightOf(input, id, room, hardCap));
  const hs = share(nat, ids.map((id) => pinned(input, id) != null), ids.map((id) => grows(input, id)), room - GAP * (ids.length - 1), ids.map((id, i) => floorOf(input, id, nat[i])))
    .map((h, i) => Math.max(h, floorOf(input, ids[i], nat[i])));
  return hs.map((h, i) => ({ h, extra: grows(input, ids[i]) ? h - nat[i] : 0 }));
};
const stack = (ids: readonly string[], boxes: readonly Box[], put: (id: string, y: number, box: Box) => void, y0 = 0) => {
  let y = y0;
  ids.forEach((id, i) => { put(id, y, boxes[i]); y += boxes[i].h + GAP; });
};

export const stageRects = (input: LayoutInput, narrow: boolean): Map<string, StageRect> => {
  const out = new Map<string, StageRect>();
  const g = input.state.stage;
  const { ids, snap } = g;
  const H = input.height;
  if (!ids.length) return out;
  if (narrow) {
    const front = ids.includes(input.front ?? '') ? input.front! : (ids.find((id) => !input.pip.has(id)) ?? ids[0]);
    const pips = ids.filter((id) => id !== front && input.pip.has(id));
    const top = ids.length - pips.length > 1 ? TAB_H : 0;
    const contentOf = (id: string, cap: number) => Math.min(cap, Math.max(120, input.contentH[id] ?? DEFAULT_H));
    const pipH = pips.map((id) => contentOf(id, H * 0.8));
    const pad = pips.length ? Math.max(...pipH) * (PIP_W / PIP_LAYOUT_W) + PIP_INSET * 2 : 0;
    const frontH = Math.min(H - top, Math.max(120, (input.contentH[front] ?? DEFAULT_H) + pad));
    out.set(front, { x: 0, y: top, w: input.width, h: frontH, mode: 'stage', pad: pad || undefined });
    for (const id of ids) {
      if (id === front) continue;
      const pip = pips.indexOf(id);
      if (pip >= 0) {
        const h = pipH[pip];
        out.set(id, { x: input.width - PIP_W - PIP_INSET, y: top + frontH - h * (PIP_W / PIP_LAYOUT_W) - PIP_INSET, w: PIP_LAYOUT_W, h, mode: 'pip' });
      } else out.set(id, { x: 0, y: top, w: input.width, h: contentOf(id, H - top), mode: 'behind' });
    }
    return out;
  }
  const full = ids.length === 1 && snap === FULL_SNAP;
  const x0 = full && !input.stripOpen ? 0 : STRIP_W;
  const avail = input.width - x0;
  const ratio = ratioOf(g);
  const stage = (x: number, y: number, w: number, h: number, left: SideRole, right: SideRole): StageRect =>
    ({ x, y, w, h, mode: 'stage', sides: { left, right } });
  if (ids.length === 1) {
    const w = full ? avail : Math.min(avail, Math.max(Math.min(avail, 360), avail * ratio));
    const [box] = fitColumn(input, ids, H);
    out.set(ids[0], { ...stage(x0 + (avail - w) / 2, 0, w, box.h, 'sym', 'sym'), extra: box.extra || undefined });
    return out;
  }
  const groupW = avail * spanOf(g);
  const gx = x0 + (avail - groupW) / 2;
  const span = groupW - GAP;
  const put = (id: string, x: number, y: number, w: number, box: Box, left: SideRole, right: SideRole) =>
    out.set(id, { ...stage(x, y, w, box.h, left, right), extra: box.extra || undefined });
  if (ids.length === 2) {
    const w1 = span * ratio;
    put(ids[0], gx, 0, w1, fitColumn(input, [ids[0]], H)[0], 'outer', 'seam');
    put(ids[1], gx + w1 + GAP, 0, span - w1, fitColumn(input, [ids[1]], H)[0], 'seam', 'outer');
    return out;
  }
  if ((ids.length === 3 && snap === TOP_SNAP) || (ids.length === 4 && snap === ROW_SNAP)) {
    const [top, ...bottom] = ids;
    const nat0 = heightOf(input, top, H * 0.6, H * 0.8);
    const bottomNat = Math.max(...bottom.map((id) => heightOf(input, id, H * 0.6, H)));
    const h0 = grows(input, top) && !bottom.some((id) => grows(input, id))
      ? Math.max(nat0, Math.min(H * 0.8, H - GAP - bottomNat))
      : nat0;
    const y1 = h0 + GAP;
    const cap = Math.max(H - y1, H * 0.6);
    put(top, gx, 0, groupW, { h: h0, extra: h0 - nat0 }, 'outer', 'outer');
    if (bottom.length === 2) {
      const w1 = span * ratio;
      put(bottom[0], gx, y1, w1, fitColumn(input, [bottom[0]], cap, H)[0], 'outer', 'seam');
      put(bottom[1], gx + w1 + GAP, y1, span - w1, fitColumn(input, [bottom[1]], cap, H)[0], 'seam', 'outer');
      return out;
    }
    const row = groupW - 2 * GAP;
    const [c1, c2] = splitsOf(g);
    const widths = [c1 * row, (c2 - c1) * row, (1 - c2) * row];
    let x = gx;
    bottom.forEach((id, i) => {
      put(id, x, y1, widths[i], fitColumn(input, [id], cap, H)[0], i === 0 ? 'outer' : 'seam', i === 2 ? 'outer' : 'seam');
      x += widths[i] + GAP;
    });
    return out;
  }
  if (ids.length === 4 && snap === GRID_SNAP) {
    const w1 = span * ratio;
    const rows = [[ids[0], ids[1]], [ids[2], ids[3]]];
    const nat = rows.map((pair) => pair.map((id) => heightOf(input, id, H)));
    const rowH = share(
      nat.map((pair) => Math.max(...pair)),
      rows.map((pair) => pair.every((id) => pinned(input, id) != null)),
      rows.map((pair) => pair.some((id) => grows(input, id))),
      H - GAP,
      rows.map((pair, r) => Math.max(...pair.map((id, c) => (grows(input, id) ? floorOf(input, id, nat[r][c]) : nat[r][c])))),
    );
    let y = 0;
    rows.forEach((pair, r) => {
      pair.forEach((id, c) => {
        const h = Math.max(floorOf(input, id, nat[r][c]), grows(input, id) ? rowH[r] : Math.min(nat[r][c], rowH[r]));
        put(id, c ? gx + w1 + GAP : gx, y, c ? span - w1 : w1, { h, extra: grows(input, id) ? h - nat[r][c] : 0 }, c ? 'seam' : 'outer', c ? 'outer' : 'seam');
      });
      y += rowH[r] + GAP;
    });
    return out;
  }
  const right = ids.length === 3 && snap === 1;
  const mainW = span * ratio;
  const colW = span - mainW;
  const mainX = right ? gx + colW + GAP : gx;
  const colX = right ? gx : gx + mainW + GAP;
  const mainSides: [SideRole, SideRole] = right ? ['seam', 'outer'] : ['outer', 'seam'];
  const colSides: [SideRole, SideRole] = right ? ['outer', 'seam'] : ['seam', 'outer'];
  const rest = ids.slice(1);
  const restBoxes = fitColumn(input, rest, H);
  const colBottom = restBoxes.reduce((sum, box) => sum + box.h, 0) + GAP * (rest.length - 1);
  const main = fitColumn(input, [ids[0]], H)[0];
  const mainBox = grows(input, ids[0]) && colBottom > main.h ? { h: colBottom, extra: main.extra + colBottom - main.h } : main;
  put(ids[0], mainX, 0, mainW, mainBox, ...mainSides);
  stack(rest, restBoxes, (id, y, box) => put(id, colX, y, colW, box, ...colSides));
  return out;
};

const thumbBox = (input: LayoutInput, id: string, visualCap = Infinity) => {
  const w = input.lastW[id] ?? DEFAULT_W;
  const content = input.contentH[id] ?? DEFAULT_H;
  const h = Math.min(content, w * THUMB_CROP, (visualCap * w) / THUMB_W);
  return { w, h, crop: content > h + 1 };
};
const THUMB_MIN_H = 56;

const stripLayout = (input: LayoutInput, shown: boolean, zBase: number, placements: Map<string, Placement>) => {
  const groups = input.state.strip;
  if (!groups.length) return;
  const cap = Math.max(THUMB_MIN_H, (input.height - 8) / groups.length - STRIP_GAP);
  const slots = groups.map((g) => {
    const box = thumbBox(input, g.ids[0], cap);
    const scale = THUMB_W / box.w;
    return { box, scale, visual: box.h * scale + STRIP_GAP + (g.ids.length - 1) * 6 };
  });
  const total = slots.reduce((sum, slot) => sum + slot.visual, 0);
  const squeeze = total > input.height && groups.length > 1
    ? (input.height - slots[slots.length - 1].visual) / (total - slots[slots.length - 1].visual)
    : 1;
  let y = 8;
  groups.forEach((group, index) => {
    const slot = slots[index];
    const x = shown ? THUMB_X : -THUMB_W - 80;
    group.ids.forEach((id, depth) => {
      const box = depth === 0 ? slot.box : thumbBox(input, id, cap);
      const scale = THUMB_W / box.w;
      placements.set(id, {
        x: r1(x + depth * 7), y: r1(y + depth * 6), w: r1(box.w), h: r1(box.h), scale, tilt: TILT,
        z: zBase + index * 4 - depth, opacity: shown ? (depth === 0 ? 1 : 0.92) : 0, mode: 'strip', crop: box.crop || undefined,
      });
    });
    y += slot.visual * squeeze;
  });
};

const classicLayout = (input: LayoutInput, placements: Map<string, Placement>): number => {
  const W = input.width;
  const column = (id: string): Column => input.columns?.[id] ?? 'main';
  const two = W >= CLASSIC_TWO_COL;
  const sideW = two ? Math.max(310, ((W - GAP) * 0.72) / 2.12) : W;
  const mainW = two ? W - GAP - sideW : W;
  const put = (id: string, x: number, y: number, w: number) => {
    const h = Math.max(60, input.contentH[id] ?? DEFAULT_H);
    placements.set(id, { ...at(x, y, w, h, 'classic', 100) });
    return h + GAP;
  };
  let y = 0;
  for (const id of input.cards) if (column(id) === 'full') y += put(id, 0, y, W);
  const order = (c: Column) => input.cards.filter((id) => column(id) === c);
  if (!two) {
    for (const id of [...order('main'), ...order('side'), ...order('float')]) y += put(id, 0, y, W);
    return Math.max(0, y - GAP);
  }
  let mainY = y;
  let sideY = y;
  for (const id of order('main')) mainY += put(id, 0, mainY, mainW);
  for (const id of order('side')) sideY += put(id, mainW + GAP, sideY, sideW);
  for (const id of order('float')) {
    if (mainY <= sideY) mainY += put(id, 0, mainY, mainW);
    else sideY += put(id, mainW + GAP, sideY, sideW);
  }
  return Math.max(0, Math.max(mainY, sideY) - GAP);
};

export const solve = (input: LayoutInput): Layout => {
  const placements = new Map<string, Placement>();
  if (input.view === 'classic') {
    const canvasH = classicLayout(input, placements);
    return { placements, narrow: input.width < NARROW_W, classic: true, stripShown: false, stripAuto: false, stageX: 0, canvasH, tabs: [] };
  }
  const narrow = input.width < NARROW_W;
  const stage = stageRects(input, narrow);
  const full = input.state.stage.ids.length === 1 && input.state.stage.snap === FULL_SNAP;
  const stripAuto = narrow || full;
  const stripShown = !stripAuto || input.stripOpen;
  let canvasH = input.height;
  const tabs = narrow ? input.state.stage.ids.filter((id) => !input.pip.has(id)) : [];

  for (const [id, rect] of stage) {
    const z = rect.mode === 'pip' ? 210 : rect.mode === 'behind' ? 150 : 200;
    const p = at(rect.x, rect.y, rect.w, rect.h, rect.mode, z);
    if (rect.pad) p.pad = r1(rect.pad);
    if (rect.sides) p.sides = rect.sides;
    if (rect.mode === 'stage' && !narrow && input.state.stage.heights?.[id] != null) p.fill = true;
    else if (rect.mode === 'stage' && !narrow && rect.extra && Math.abs(rect.extra) >= 1) p.extra = Math.round(rect.extra);
    if (rect.mode === 'pip') p.scale = PIP_W / PIP_LAYOUT_W;
    if (rect.mode === 'behind') p.opacity = 0;
    placements.set(id, p);
    canvasH = Math.max(canvasH, p.y + p.h);
  }
  stripLayout(input, stripShown, narrow ? 300 : 100, placements);
  for (const id of input.state.hidden) {
    const box = thumbBox(input, id);
    placements.set(id, { x: -THUMB_W - 80, y: 8, w: box.w, h: box.h, scale: THUMB_W / box.w, tilt: TILT, z: 0, opacity: 0, mode: 'hidden' });
  }
  const stageX = narrow || (full && !input.stripOpen) ? 0 : STRIP_W;
  return { placements, narrow, classic: false, stripShown, stripAuto, stageX, canvasH: Math.ceil(canvasH), tabs: tabs.length > 1 ? tabs : [] };
};

export const transformOf = (p: Pick<Placement, 'x' | 'y' | 'h' | 'scale' | 'tilt'>): string => {
  const half = r1((p.h * p.scale) / 2);
  return `translate(${p.x}px, ${r1(p.y + half)}px) perspective(${p.tilt ? STRIP_PERSPECTIVE : PERSPECTIVE}px) rotateY(${p.tilt}deg) translate(0px, ${-half}px) scale(${Math.round(p.scale * 10000) / 10000})`;
};
