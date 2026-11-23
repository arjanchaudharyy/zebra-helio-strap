export interface StageGroup {
  ids: string[];
  snap: number;
  ratio?: number;
  splits?: number[];
  span?: number;
  heights?: Record<string, number>;
}
export interface StageState {
  stage: StageGroup;
  strip: StageGroup[];
  hidden: string[];
}

export const MAX_ON_STAGE = 4;
export const FULL_SNAP = 2;
export const snapCount = (_cards: number): number => 3;
export const TOP_SNAP = 2;
export const GRID_SNAP = 0;
export const ROW_SNAP = 1;
export const COLUMN_SNAP = 2;

export const snapFor = (ids: readonly string[], wide?: (id: string) => boolean): number => {
  const lead = ids[0] != null && wide?.(ids[0]);
  if (ids.length === 3) return lead ? TOP_SNAP : 0;
  if (ids.length === 4) return lead ? ROW_SNAP : GRID_SNAP;
  return 0;
};
const heightsOf = (g: { heights?: Record<string, number> }, ids: readonly string[]): Record<string, number> | undefined => {
  if (!g.heights) return undefined;
  const out: Record<string, number> = {};
  for (const id of ids) {
    const h = g.heights[id];
    if (typeof h === 'number' && Number.isFinite(h)) out[id] = h;
  }
  return Object.keys(out).length ? out : undefined;
};
const make = (ids: string[], snap: number, extra: Partial<StageGroup> = {}): StageGroup => {
  const out: StageGroup = { ids: [...ids], snap };
  if (extra.ratio != null && Number.isFinite(extra.ratio)) out.ratio = extra.ratio;
  if (extra.span != null && Number.isFinite(extra.span)) out.span = extra.span;
  if (ids.length === 4 && Array.isArray(extra.splits) && extra.splits.length === 2 && extra.splits.every(Number.isFinite)) out.splits = [...extra.splits];
  const heights = heightsOf(extra, ids);
  if (heights) out.heights = heights;
  return out;
};
const group = (g: StageGroup): StageGroup => make(g.ids, g.snap, g);
const copy = (s: StageState): StageState => ({
  stage: group(s.stage),
  strip: s.strip.map(group),
  hidden: [...s.hidden],
});

const clampSnap = (g: StageGroup): StageGroup =>
  make(g.ids, Math.min(Math.max(0, Math.round(g.snap) || 0), snapCount(g.ids.length) - 1), g);

const take = (s: StageState, id: string): StageState => {
  const out = copy(s);
  if (out.stage.ids.includes(id)) out.stage = clampSnap({ ids: out.stage.ids.filter((x) => x !== id), snap: 0, heights: out.stage.heights });
  out.strip = out.strip
    .map((g) => (g.ids.includes(id) ? clampSnap({ ids: g.ids.filter((x) => x !== id), snap: 0, heights: g.heights }) : g))
    .filter((g) => g.ids.length > 0);
  out.hidden = out.hidden.filter((x) => x !== id);
  return out;
};

const fill = (s: StageState): StageState => {
  if (s.stage.ids.length || !s.strip.length) return s;
  const [next, ...rest] = s.strip;
  return { ...s, stage: next, strip: rest };
};

export type Where = { at: 'stage'; index: number } | { at: 'strip'; group: number; index: number } | { at: 'hidden' } | null;

export const locate = (s: StageState, id: string): Where => {
  const onStage = s.stage.ids.indexOf(id);
  if (onStage >= 0) return { at: 'stage', index: onStage };
  for (let group = 0; group < s.strip.length; group += 1) {
    const index = s.strip[group].ids.indexOf(id);
    if (index >= 0) return { at: 'strip', group, index };
  }
  return s.hidden.includes(id) ? { at: 'hidden' } : null;
};

export const bringGroup = (s: StageState, group: number): StageState => {
  const picked = s.strip[group];
  if (!picked) return s;
  const out = copy(s);
  out.strip.splice(group, 1);
  if (out.stage.ids.length) out.strip.unshift(clampSnap(out.stage));
  out.stage = clampSnap(picked);
  return out;
};

export const cycle = (s: StageState, dir: 1 | -1): StageState => {
  if (!s.strip.length) return s;
  const out = copy(s);
  if (dir > 0) {
    const next = out.strip.shift()!;
    out.strip.push(out.stage);
    out.stage = next;
  } else {
    const next = out.strip.pop()!;
    out.strip.unshift(out.stage);
    out.stage = next;
  }
  return out;
};

export const addToStage = (s: StageState, id: string, at?: number, wide?: (id: string) => boolean): StageState => {
  if (s.stage.ids.includes(id)) {
    if (at == null) return s;
    const ids = s.stage.ids.filter((x) => x !== id);
    ids.splice(Math.min(Math.max(0, at), ids.length), 0, id);
    return { ...copy(s), stage: make(ids, s.stage.snap, s.stage) };
  }
  const out = take(s, id);
  const ids = [...out.stage.ids];
  if (ids.length >= MAX_ON_STAGE) {
    const slot = Math.min(Math.max(0, at ?? ids.length - 1), ids.length - 1);
    const [replaced] = ids.splice(slot, 1, id);
    out.strip.unshift({ ids: [replaced], snap: 0 });
    out.stage = make(ids, out.stage.snap, { heights: out.stage.heights });
    return out;
  }
  ids.splice(Math.min(Math.max(0, at ?? ids.length), ids.length), 0, id);
  out.stage = make(ids, snapFor(ids, wide), { heights: out.stage.heights });
  return out;
};

export const removeFromStage = (s: StageState, id: string, wide?: (id: string) => boolean): StageState => {
  if (!s.stage.ids.includes(id)) return s;
  if (s.stage.ids.length === 1) {
    if (!s.strip.length) return s;
    const out = copy(s);
    const next = out.strip.shift()!;
    out.strip.unshift({ ids: [id], snap: out.stage.snap === FULL_SNAP ? 0 : out.stage.snap });
    out.stage = next;
    return out;
  }
  const out = copy(s);
  const ids = out.stage.ids.filter((x) => x !== id);
  out.stage = make(ids, snapFor(ids, wide), { heights: out.stage.heights });
  out.strip.unshift({ ids: [id], snap: 0 });
  return out;
};

export const solo = (s: StageState, id: string, snap = 0): StageState => {
  const where = locate(s, id);
  if (!where) return s;
  const others = s.stage.ids.filter((x) => x !== id);
  const out = take(s, id);
  out.strip = out.strip.filter((g) => g.ids.length > 0);
  if (where.at === 'stage') {
    out.stage = { ids: [], snap: 0 };
    if (others.length) out.strip.unshift(clampSnap({ ids: others, snap: 0 }));
  } else if (out.stage.ids.length) {
    out.strip.unshift(clampSnap(out.stage));
  }
  out.stage = clampSnap({ ids: [id], snap });
  return out;
};

export const fullscreen = (s: StageState, id: string): StageState => solo(s, id, FULL_SNAP);

export const setSnap = (s: StageState, snap: number): StageState =>
  ({ ...copy(s), stage: clampSnap(make(s.stage.ids, snap, { heights: s.stage.heights })) });

export interface SizePatch { ratio?: number; span?: number; splits?: number[]; heights?: Record<string, number | null> }
const round3 = (n: number) => Math.round(n * 1000) / 1000;
export const setSize = (s: StageState, patch: SizePatch): StageState => {
  const g = s.stage;
  const snap = g.ids.length === 1 && g.snap === FULL_SNAP && (patch.ratio != null || patch.span != null) ? 0 : g.snap;
  const heights: Record<string, number> = { ...(g.heights ?? {}) };
  for (const [id, h] of Object.entries(patch.heights ?? {})) {
    if (h == null) delete heights[id];
    else heights[id] = Math.round(h);
  }
  const extra: Partial<StageGroup> = {
    ratio: patch.ratio != null ? round3(patch.ratio) : g.ratio,
    span: patch.span != null ? (patch.span >= 0.999 ? undefined : round3(patch.span)) : g.span,
    splits: patch.splits ? patch.splits.map(round3) : g.splits,
    heights,
  };
  return { ...copy(s), stage: clampSnap(make(g.ids, snap, extra)) };
};

export const setRatio = (s: StageState, ratio: number): StageState => setSize(s, { ratio });

export const swapOnStage = (s: StageState, a: string, b: string): StageState => {
  const ids = [...s.stage.ids];
  const [i, j] = [ids.indexOf(a), ids.indexOf(b)];
  if (i < 0 || j < 0 || i === j) return s;
  [ids[i], ids[j]] = [ids[j], ids[i]];
  return { ...copy(s), stage: clampSnap(make(ids, s.stage.snap, s.stage)) };
};

export const hide = (s: StageState, id: string): StageState => {
  if (!locate(s, id) || s.hidden.includes(id)) return s;
  const out = take(s, id);
  out.hidden.push(id);
  return fill(out);
};

export const focusCard = (s: StageState, id: string): StageState => {
  const where = locate(s, id);
  if (!where || where.at === 'stage') return s;
  if (where.at === 'strip') return bringGroup(s, where.group);
  return solo({ ...copy(s), hidden: s.hidden.filter((x) => x !== id), strip: [{ ids: [id], snap: 0 }, ...copy(s).strip] }, id);
};
